#![allow(dead_code)]
//! ═══════════════════════════════════════════════════════════════
//! Bankroll Management & Kelly Criterion Calculator
//!
//! Provides position sizing recommendations based on the
//! Kelly Criterion formula: f* = (bp - q) / b
//! where:
//!   f* = fraction of bankroll to wager
//!   b  = decimal odds - 1 (net received on a win)
//!   p  = probability of winning
//!   q  = probability of losing (1 - p)
//!
//! Also provides flat-staking, percentage-staking, confidence-adjusted, and
//! volatility-adjusted Kelly variants.
//! ═══════════════════════════════════════════════════════════════

use chrono::{Datelike, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const BANKROLL_DIR: &str = ".openclaw/kalshi-monster";
const BANKROLL_FILE: &str = "bankroll.json";

// ═══════════════════════════════════════════════════════════════
// Core Types
// ═══════════════════════════════════════════════════════════════

/// Bankroll configuration and state
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BankrollConfig {
    /// Total current bankroll in dollars
    pub total_bankroll: f64,
    /// Initial bankroll (for ROI calculation)
    pub initial_bankroll: f64,
    /// Kelly fraction: 1.0 = full Kelly, 0.25 = quarter Kelly, etc.
    pub kelly_fraction: f64,
    /// Maximum single bet as fraction of bankroll (e.g., 0.05 = 5%)
    pub max_bet_pct: f64,
    /// Minimum single bet in dollars
    pub min_bet: f64,
    /// Default odds format (American odds, e.g. -110 for standard PrizePicks)
    pub default_odds: f64,
    /// Staking strategy
    pub strategy: StakingStrategy,
    /// Per-player risk adjustments (player_name -> risk_multiplier)
    pub player_risk_multipliers: HashMap<String, f64>,
    /// Track daily/weekly exposure
    pub daily_bet_limit: f64,
    pub weekly_bet_limit: f64,
    /// Historical Brier score from graded LLM forecasts (0.0 if no history yet).
    /// Used for P3 volatility-adjusted Kelly. When >0, reduces sizing for poor historical calibration.
    #[serde(default)]
    pub historical_brier: f64,
}

impl Default for BankrollConfig {
    fn default() -> Self {
        Self {
            total_bankroll: 1000.0,
            initial_bankroll: 1000.0,
            kelly_fraction: 0.25,    // Quarter Kelly is standard recommendation
            max_bet_pct: 0.05,       // Max 5% of bankroll on single bet
            min_bet: 5.0,            // Minimum $5 bet
            default_odds: -110.0,    // Standard US odds
            strategy: StakingStrategy::Kelly,
            player_risk_multipliers: HashMap::new(),
            daily_bet_limit: 200.0,
            weekly_bet_limit: 500.0,
            historical_brier: 0.0,
        }
    }
}

/// Staking strategy for bet sizing
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum StakingStrategy {
    /// Kelly Criterion
    Kelly,
    /// Fixed dollar amount per bet
    FlatBet,
    /// Fixed percentage of current bankroll
    PercentageOfBankroll,
    /// Confidence-adjusted Kelly (scale Kelly by confidence / 100)
    ConfidenceAdjustedKelly,
    /// Volatility-adjusted Kelly from historical Brier score (P3: shrinks when calibration poor)
    VolatilityAdjustedKelly,
}

impl std::fmt::Display for StakingStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StakingStrategy::Kelly => write!(f, "Kelly Criterion"),
            StakingStrategy::FlatBet => write!(f, "Flat Bet"),
            StakingStrategy::PercentageOfBankroll => write!(f, "% of Bankroll"),
            StakingStrategy::ConfidenceAdjustedKelly => write!(f, "Confidence-Adjusted Kelly"),
            StakingStrategy::VolatilityAdjustedKelly => write!(f, "Volatility-Adjusted Kelly"),
        }
    }
}

impl std::str::FromStr for StakingStrategy {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "kelly" => Ok(StakingStrategy::Kelly),
            "flat" | "flatbet" | "flat_bet" => Ok(StakingStrategy::FlatBet),
            "percentage" | "percentageofbankroll" | "percentage_of_bankroll" => {
                Ok(StakingStrategy::PercentageOfBankroll)
            }
            "confidence_adjusted" | "confidenceadjustedkelly" | "conf_adjusted" => {
                Ok(StakingStrategy::ConfidenceAdjustedKelly)
            }
            "volatility" | "volatilityadjustedkelly" | "vol_adjusted" | "volatility_adjusted" => {
                Ok(StakingStrategy::VolatilityAdjustedKelly)
            }
            _ => Err(format!("Unknown staking strategy: {}", s)),
        }
    }
}

/// A single bet recommendation
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BetRecommendation {
    pub player_name: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String,
    pub win_probability: f64,
    pub confidence_score: Option<u8>,
    pub recommended_stake: f64,
    pub kelly_pct: f64,
    pub expected_value: f64,
    pub expected_profit: f64,
    pub risk_level: String,
    pub notes: String,
}

/// Summary of bankroll status
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BankrollSummary {
    pub config: BankrollConfig,
    pub roi_pct: f64,
    pub total_wagered: f64,
    pub total_won: f64,
    pub total_lost: f64,
    pub profit_loss: f64,
    pub net_profit: f64,
    pub current_bankroll: f64,
    pub bets_placed: f64,
    pub win_rate: f64,
    pub bets_today: f64,
    pub bets_this_week: f64,
    pub remaining_daily: f64,
    pub remaining_weekly: f64,
    pub daily_limit_used: f64,
    pub weekly_limit_used: f64,
    pub prediction_open_exposure: f64,
    pub paper_open_exposure: f64,
    pub paper_cash_balance: f64,
    pub paper_realized_pnl: f64,
    pub synced_at: String,
}

/// Parlay-specific bet sizing with correlation adjustment
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParlayRecommendation {
    pub legs: Vec<BetRecommendation>,
    pub combined_probability: f64,
    pub correlation_factor: f64,
    pub recommended_stake: f64,
    pub expected_value: f64,
    pub max_payout: f64,
    pub risk_level: String,
}

// ═══════════════════════════════════════════════════════════════
// Kelly Criterion Engine
// ═══════════════════════════════════════════════════════════════

/// Calculate the Kelly Criterion bet size
/// Returns the fraction of bankroll to wager
pub fn kelly_criterion(win_probability: f64, decimal_odds: f64) -> f64 {
    if win_probability <= 0.0 || win_probability >= 1.0 || decimal_odds <= 1.0 {
        return 0.0;
    }
    let b = decimal_odds - 1.0; // net profit per unit wagered
    let p = win_probability;
    let q = 1.0 - p;
    let kelly = (b * p - q) / b;
    kelly.max(0.0)
}

/// Volatility-adjusted Kelly using historical Brier score of past LLM forecasts.
/// This is the start of P3 implementation: when historical_brier > 0 (accumulated graded data),
/// we shrink the Kelly fraction to account for realized volatility / miscalibration.
/// Simple linear penalty for now; can be refined with more data.
pub fn volatility_adjusted_kelly(win_probability: f64, decimal_odds: f64, historical_brier: f64) -> f64 {
    let base = kelly_criterion(win_probability, decimal_odds);
    if historical_brier <= 0.0 || historical_brier > 1.0 {
        return base;
    }
    // Penalty: brier~0.1 (excellent) -> factor ~1.4 , brier 0.25 -> factor ~2.0 (more conservative)
    let penalty = 1.0 + historical_brier * 4.0;
    (base / penalty).max(0.0)
}

/// Convert American odds to decimal odds
pub fn american_to_decimal(american_odds: f64) -> f64 {
    if american_odds > 0.0 {
        1.0 + (american_odds / 100.0)
    } else if american_odds < 0.0 {
        1.0 - (100.0 / american_odds)
    } else {
        2.0 // Even money
    }
}

/// Calculate expected value of a bet
pub fn expected_value(win_probability: f64, decimal_odds: f64, stake: f64) -> f64 {
    let win_amount = stake * (decimal_odds - 1.0);
    let loss_amount = stake;
    win_probability * win_amount - (1.0 - win_probability) * loss_amount
}

/// Generate a bet recommendation for a single pick
pub fn recommend_bet(
    config: &BankrollConfig,
    player_name: &str,
    prop_category: &str,
    line: f64,
    pick_type: &str,
    win_probability_pct: f64,
    confidence_score: Option<u8>,
) -> BetRecommendation {
    let prob = win_probability_pct / 100.0;
    let decimal_odds = american_to_decimal(config.default_odds);

    // Apply player-specific risk multiplier
    let risk_mult = config
        .player_risk_multipliers
        .get(player_name)
        .copied()
        .unwrap_or(1.0);

    // Calculate raw Kelly percentage
    let raw_kelly = kelly_criterion(prob, decimal_odds);

    // Apply Kelly fraction and risk multiplier
    let kelly_pct = raw_kelly * config.kelly_fraction * risk_mult;

    // Calculate stake based on strategy
    let stake = match config.strategy {
        StakingStrategy::Kelly => {
            let kelly_stake = config.total_bankroll * kelly_pct;
            kelly_stake.clamp(config.min_bet, config.total_bankroll * config.max_bet_pct)
        }
        StakingStrategy::FlatBet => {
            let flat_amount = config.total_bankroll * 0.02; // 2% default flat
            flat_amount.clamp(config.min_bet, config.total_bankroll * config.max_bet_pct)
        }
        StakingStrategy::PercentageOfBankroll => {
            let pct = 0.02; // 2% default
            let stake = config.total_bankroll * pct * risk_mult;
            stake.clamp(config.min_bet, config.total_bankroll * config.max_bet_pct)
        }
        StakingStrategy::ConfidenceAdjustedKelly => {
            let conf_mult = confidence_score.map(|c| c as f64 / 100.0).unwrap_or(0.5);
            let adjusted_kelly = kelly_pct * conf_mult;
            let stake = config.total_bankroll * adjusted_kelly;
            stake.clamp(config.min_bet, config.total_bankroll * config.max_bet_pct)
        }
        StakingStrategy::VolatilityAdjustedKelly => {
            let vol_kelly = volatility_adjusted_kelly(prob, decimal_odds, config.historical_brier);
            let adjusted_kelly = vol_kelly * config.kelly_fraction * risk_mult;
            let stake = config.total_bankroll * adjusted_kelly;
            stake.clamp(config.min_bet, config.total_bankroll * config.max_bet_pct)
        }
    };

    let ev = expected_value(prob, decimal_odds, stake);
    let expected_profit = ev;

    let risk_level = if kelly_pct > 0.03 {
        "High".to_string()
    } else if kelly_pct > 0.01 {
        "Medium".to_string()
    } else if kelly_pct > 0.0 {
        "Low".to_string()
    } else {
        "No Edge".to_string()
    };

    let edge_pct = (prob * decimal_odds - 1.0) * 100.0;

    let notes = if raw_kelly <= 0.0 {
        format!(
            "Negative edge (prob {:.1}% vs required {:.1}%). Skip this pick.",
            win_probability_pct,
            (1.0 / decimal_odds) * 100.0
        )
    } else {
        format!(
            "Edge: {:.1}%. Raw Kelly: {:.1}%, Fractional Kelly: {:.1}%",
            edge_pct,
            raw_kelly * 100.0,
            kelly_pct * 100.0
        )
    };

    BetRecommendation {
        player_name: player_name.to_string(),
        prop_category: prop_category.to_string(),
        line,
        pick_type: pick_type.to_string(),
        win_probability: win_probability_pct,
        confidence_score,
        recommended_stake: (stake * 100.0).round() / 100.0,
        kelly_pct: (kelly_pct * 10000.0).round() / 100.0, // as percentage with 2 decimals
        expected_value: (ev * 100.0).round() / 100.0,
        expected_profit: (expected_profit * 100.0).round() / 100.0,
        risk_level,
        notes,
    }
}

/// Generate recommendations for multiple picks, sorted by expected value
pub fn recommend_multiple_bets(
    config: &BankrollConfig,
    picks: &[PickInput],
) -> Vec<BetRecommendation> {
    let mut recommendations: Vec<BetRecommendation> = picks
        .iter()
        .map(|pick| {
            recommend_bet(
                config,
                &pick.player_name,
                &pick.prop_category,
                pick.line,
                &pick.pick_type,
                pick.win_probability,
                pick.confidence_score,
            )
        })
        .filter(|r| r.kelly_pct > 0.0 && r.risk_level != "No Edge")
        .collect();

    // Sort by expected value (descending)
    recommendations.sort_by(|a, b| b.expected_value.partial_cmp(&a.expected_value).unwrap());
    recommendations
}

/// Input for generating bet recommendations
#[derive(Debug, Clone)]
pub struct PickInput {
    pub player_name: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String,
    pub win_probability: f64,
    pub confidence_score: Option<u8>,
}

// ═══════════════════════════════════════════════════════════════
// Parlay Sizing with Correlation Adjustment
// ═══════════════════════════════════════════════════════════════

/// Calculate parlay probability and recommend stake
pub fn recommend_parlay(
    config: &BankrollConfig,
    legs: &[PickInput],
    correlation_factor: f64, // 0.0 = fully correlated, 1.0 = independent
) -> ParlayRecommendation {
    if legs.is_empty() {
        return ParlayRecommendation {
            legs: vec![],
            combined_probability: 0.0,
            correlation_factor,
            recommended_stake: 0.0,
            expected_value: 0.0,
            max_payout: 0.0,
            risk_level: "Invalid".to_string(),
        };
    }

    // Get individual leg recommendations
    let leg_recs: Vec<BetRecommendation> = legs
        .iter()
        .map(|pick| {
            recommend_bet(
                config,
                &pick.player_name,
                &pick.prop_category,
                pick.line,
                &pick.pick_type,
                pick.win_probability,
                pick.confidence_score,
            )
        })
        .collect();

    // Calculate combined probability (with correlation adjustment)
    let raw_combined: f64 = legs.iter().map(|l| l.win_probability / 100.0).product();
    // Correlation adjustment: blend between fully correlated (max prob) and independent (product)
    let max_single_prob = legs.iter().map(|l| l.win_probability / 100.0).fold(0.0, f64::max);
    let adjusted_combined =
        correlation_factor * raw_combined + (1.0 - correlation_factor) * max_single_prob;

    // Parlay odds (approximate for standard -110 legs)
    let n_legs = legs.len() as f64;
    let decimal_per_leg = american_to_decimal(config.default_odds);
    let parlay_decimal_odds = decimal_per_leg.powf(n_legs);

    // Kelly for parlay (much smaller due to compounding risk)
    let parlay_kelly = kelly_criterion(adjusted_combined, parlay_decimal_odds);
    let stake = config.total_bankroll
        * parlay_kelly
        * config.kelly_fraction
        * 0.5; // Half Kelly for parlays
    let stake = stake.clamp(0.0, config.total_bankroll * config.max_bet_pct * 0.5);

    let ev = expected_value(adjusted_combined, parlay_decimal_odds, stake);
    let max_payout = stake * (parlay_decimal_odds - 1.0);

    let risk_level = if legs.len() >= 4 {
        "Very High".to_string()
    } else if legs.len() >= 3 {
        "High".to_string()
    } else {
        "Medium".to_string()
    };

    ParlayRecommendation {
        legs: leg_recs,
        combined_probability: (adjusted_combined * 10000.0).round() / 100.0,
        correlation_factor,
        recommended_stake: (stake * 100.0).round() / 100.0,
        expected_value: (ev * 100.0).round() / 100.0,
        max_payout: (max_payout * 100.0).round() / 100.0,
        risk_level,
    }
}

// ═══════════════════════════════════════════════════════════════
// Bankroll Persistence
// ═══════════════════════════════════════════════════════════════

fn bankroll_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(BANKROLL_DIR).join(BANKROLL_FILE)
}

pub fn load_bankroll_config() -> BankrollConfig {
    let path = bankroll_path();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str::<BankrollConfig>(&content) {
                return config;
            }
        }
    }
    let config = BankrollConfig::default();
    let _ = save_bankroll_config(&config);
    config
}

pub fn save_bankroll_config(config: &BankrollConfig) -> Result<(), String> {
    let path = bankroll_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {}", e))?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write: {}", e))?;
    Ok(())
}

/// Update bankroll after a bet result
pub fn record_result(
    config: &mut BankrollConfig,
    stake: f64,
    won: bool,
    odds: Option<f64>,
) {
    let decimal = odds.map(american_to_decimal).unwrap_or_else(|| american_to_decimal(config.default_odds));
    if won {
        config.total_bankroll += stake * (decimal - 1.0);
    } else {
        config.total_bankroll -= stake;
    }
}

/// Get a summary of bankroll status
pub fn get_bankroll_summary(config: &BankrollConfig) -> BankrollSummary {
    let profit_loss = config.total_bankroll - config.initial_bankroll;
    let roi_pct = if config.initial_bankroll > 0.0 {
        (profit_loss / config.initial_bankroll) * 100.0
    } else {
        0.0
    };

    BankrollSummary {
        config: config.clone(),
        roi_pct: (roi_pct * 100.0).round() / 100.0,
        total_wagered: 0.0,  // Would be tracked via bet history
        total_won: 0.0,
        total_lost: 0.0,
        profit_loss: (profit_loss * 100.0).round() / 100.0,
        net_profit: (profit_loss * 100.0).round() / 100.0,
        current_bankroll: config.total_bankroll,
        bets_placed: 0.0,
        win_rate: 0.0,
        bets_today: 0.0,
        bets_this_week: 0.0,
        remaining_daily: config.daily_bet_limit,
        remaining_weekly: config.weekly_bet_limit,
        daily_limit_used: 0.0,
        weekly_limit_used: 0.0,
        prediction_open_exposure: 0.0,
        paper_open_exposure: 0.0,
        paper_cash_balance: 0.0,
        paper_realized_pnl: 0.0,
        synced_at: Utc::now().to_rfc3339(),
    }
}

#[derive(Debug, Clone)]
struct BankrollSyncMetrics {
    kalshi_bets: f64,
    kalshi_total_wagered: f64,
    kalshi_total_won: f64,
    kalshi_total_lost: f64,
    kalshi_profit_loss: f64,
    kalshi_open_exposure: f64,
    kalshi_daily_wagered: f64,
    kalshi_weekly_wagered: f64,
    kalshi_wins: f64,
    kalshi_losses: f64,
    paper_total_stake: f64,
    paper_open_exposure: f64,
    paper_daily_exposure: f64,
    paper_weekly_exposure: f64,
    paper_cash_balance: f64,
    paper_realized_pnl: f64,
    paper_bets: f64,
}

fn today_start_rfc3339() -> String {
    let now = Utc::now();
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight")
        .and_utc()
        .to_rfc3339()
}

fn monday_start_rfc3339() -> String {
    let now = Utc::now();
    let days_since_monday = now.weekday().num_days_from_monday() as i64;
    let monday = now - Duration::days(days_since_monday);
    monday
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight")
        .and_utc()
        .to_rfc3339()
}

async fn fetch_kalshi_bankroll_metrics(
    pool: &Pool<Sqlite>,
    today_start: &str,
    week_start: &str,
) -> Result<(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64), String> {
    let row = sqlx::query(
        r#"
        SELECT
            COUNT(*) AS bet_count,
            COALESCE(SUM(CASE WHEN COALESCE(line, 0.0) > 0.0 THEN COALESCE(line, 0.0) ELSE 0.0 END), 0.0) AS total_wagered,
            COALESCE(SUM(CASE WHEN COALESCE(actual_result, 0.0) > 0.0 THEN actual_result ELSE 0.0 END), 0.0) AS total_won,
            COALESCE(SUM(CASE WHEN COALESCE(actual_result, 0.0) < 0.0 THEN ABS(actual_result) ELSE 0.0 END), 0.0) AS total_lost,
            COALESCE(SUM(actual_result), 0.0) AS profit_loss,
            COALESCE(SUM(CASE WHEN outcome = 'Pending' AND COALESCE(line, 0.0) > 0.0 THEN COALESCE(line, 0.0) ELSE 0.0 END), 0.0) AS open_exposure,
            COALESCE(SUM(CASE WHEN created_at >= ?1 AND COALESCE(line, 0.0) > 0.0 THEN COALESCE(line, 0.0) ELSE 0.0 END), 0.0) AS daily_wagered,
            COALESCE(SUM(CASE WHEN created_at >= ?2 AND COALESCE(line, 0.0) > 0.0 THEN COALESCE(line, 0.0) ELSE 0.0 END), 0.0) AS weekly_wagered,
            COALESCE(SUM(CASE WHEN outcome = 'Win' THEN 1 ELSE 0 END), 0.0) AS wins,
            COALESCE(SUM(CASE WHEN outcome = 'Loss' THEN 1 ELSE 0 END), 0.0) AS losses,
            COALESCE(SUM(CASE WHEN created_at >= ?1 THEN 1 ELSE 0 END), 0.0) AS daily_bets,
            COALESCE(SUM(CASE WHEN created_at >= ?2 THEN 1 ELSE 0 END), 0.0) AS weekly_bets
        FROM predictions
        WHERE full_decision_json IS NOT NULL
        "#,
    )
    .bind(today_start)
    .bind(week_start)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch prediction bankroll metrics: {}", e))?;

    Ok((
        row.get::<i64, _>("bet_count") as f64,
        row.get::<f64, _>("total_wagered"),
        row.get::<f64, _>("total_won"),
        row.get::<f64, _>("total_lost"),
        row.get::<f64, _>("profit_loss"),
        row.get::<f64, _>("open_exposure"),
        row.get::<f64, _>("daily_wagered"),
        row.get::<f64, _>("weekly_wagered"),
        row.get::<i64, _>("wins") as f64,
        row.get::<i64, _>("losses") as f64,
        row.get::<i64, _>("daily_bets") as f64,
        row.get::<i64, _>("weekly_bets") as f64,
    ))
}

async fn fetch_paper_bankroll_metrics(
    pool: &Pool<Sqlite>,
    today_start: &str,
    week_start: &str,
) -> Result<(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64), String> {
    let total_stake: f64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(stake_dollars), 0.0)
        FROM paper_lots
        WHERE COALESCE(stake_dollars, 0.0) > 0.0
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper total stake: {}", e))?;

    let open_exposure: f64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(stake_dollars), 0.0)
        FROM paper_lots
        WHERE status = 'Open' AND COALESCE(stake_dollars, 0.0) > 0.0
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper open exposure: {}", e))?;

    let daily_exposure: f64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(stake_dollars), 0.0)
        FROM paper_lots
        WHERE opened_at >= ?1
          AND COALESCE(stake_dollars, 0.0) > 0.0
        "#,
    )
    .bind(today_start)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper daily wagered: {}", e))?;

    let weekly_exposure: f64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(stake_dollars), 0.0)
        FROM paper_lots
        WHERE opened_at >= ?1
          AND COALESCE(stake_dollars, 0.0) > 0.0
        "#,
    )
    .bind(week_start)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper weekly wagered: {}", e))?;

    let paper_cash_balance: f64 = sqlx::query_scalar(
        "SELECT balance_dollars FROM paper_account WHERE id = 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper cash balance: {}", e))?
    .unwrap_or(0.0);

    let paper_realized_pnl: f64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(realized_pnl), 0.0)
        FROM paper_lots
        WHERE status = 'Closed'
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper realized PnL: {}", e))?;

    let paper_bets: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM paper_lots WHERE COALESCE(stake_dollars, 0.0) > 0.0",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper bet count: {}", e))?;

    let paper_daily_bets: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM paper_lots
        WHERE opened_at >= ?1
          AND COALESCE(stake_dollars, 0.0) > 0.0
        "#,
    )
    .bind(today_start)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper daily bets: {}", e))?;

    let paper_weekly_bets: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM paper_lots
        WHERE opened_at >= ?1
          AND COALESCE(stake_dollars, 0.0) > 0.0
        "#,
    )
    .bind(week_start)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper weekly bets: {}", e))?;

    let paper_wins: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(CASE WHEN realized_pnl > 0.0 THEN 1 ELSE 0 END), 0.0)
        FROM paper_lots
        WHERE status = 'Closed'
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper wins: {}", e))?;

    let paper_losses: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(CASE WHEN realized_pnl < 0.0 THEN 1 ELSE 0 END), 0.0)
        FROM paper_lots
        WHERE status = 'Closed'
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper losses: {}", e))?;

    Ok((
        total_stake,
        open_exposure,
        daily_exposure,
        weekly_exposure,
        paper_cash_balance,
        paper_realized_pnl,
        paper_bets as f64,
        paper_daily_bets as f64,
        paper_weekly_bets as f64,
        paper_wins as f64,
        paper_losses as f64,
    ))
}

/// Get bankroll status with live exposure synced from predictions.db and paper positions.
pub async fn get_bankroll_summary_synced(
    config: &BankrollConfig,
    pool: &Pool<Sqlite>,
) -> Result<BankrollSummary, String> {
    let today_start = today_start_rfc3339();
    let week_start = monday_start_rfc3339();
    let (
        kalshi_bets,
        kalshi_total_wagered,
        kalshi_total_won,
        kalshi_total_lost,
        kalshi_profit_loss,
        prediction_open_exposure,
        kalshi_daily_wagered,
        kalshi_weekly_wagered,
        kalshi_wins,
        kalshi_losses,
        kalshi_daily_bets,
        kalshi_weekly_bets,
    ) = fetch_kalshi_bankroll_metrics(pool, &today_start, &week_start).await?;

    let (
        paper_total_stake,
        paper_open_exposure,
        paper_daily_exposure,
        paper_weekly_exposure,
        paper_cash_balance,
        paper_realized_pnl,
        paper_bets,
        paper_daily_bets,
        paper_weekly_bets,
        paper_wins,
        paper_losses,
    ) = fetch_paper_bankroll_metrics(pool, &today_start, &week_start).await?;

    let total_wagered = kalshi_total_wagered + paper_total_stake;
    let total_won = kalshi_total_won + paper_realized_pnl.max(0.0);
    let total_lost = kalshi_total_lost + paper_realized_pnl.abs().min(paper_total_stake);
    let profit_loss = kalshi_profit_loss + paper_realized_pnl;
    let bets_placed = kalshi_bets + paper_bets;
    let wins = kalshi_wins + paper_wins;
    let losses = kalshi_losses + paper_losses;
    let win_rate = if wins + losses > 0.0 {
        wins / (wins + losses) * 100.0
    } else {
        0.0
    };
    let daily_limit_used = kalshi_daily_wagered + paper_daily_exposure;
    let weekly_limit_used = kalshi_weekly_wagered + paper_weekly_exposure;
    let remaining_daily = (config.daily_bet_limit - daily_limit_used).max(0.0);
    let remaining_weekly = (config.weekly_bet_limit - weekly_limit_used).max(0.0);
    let roi_pct = if config.initial_bankroll > 0.0 {
        profit_loss / config.initial_bankroll * 100.0
    } else {
        0.0
    };

    Ok(BankrollSummary {
        config: config.clone(),
        roi_pct: (roi_pct * 100.0).round() / 100.0,
        total_wagered,
        total_won,
        total_lost,
        profit_loss,
        net_profit: profit_loss,
        current_bankroll: (config.total_bankroll + profit_loss).max(0.0),
        bets_placed,
        win_rate,
        bets_today: kalshi_daily_bets + paper_daily_bets,
        bets_this_week: kalshi_weekly_bets + paper_weekly_bets,
        remaining_daily,
        remaining_weekly,
        daily_limit_used,
        weekly_limit_used,
        prediction_open_exposure,
        paper_open_exposure,
        paper_cash_balance,
        paper_realized_pnl,
        synced_at: Utc::now().to_rfc3339(),
    })
}

/// Return the tighter of daily/weekly remaining cap and an explanatory warning when a proposed stake exceeds it.
pub fn apply_bankroll_cap(stake: f64, summary: &BankrollSummary) -> (f64, Option<String>) {
    let cap = summary.remaining_daily.min(summary.remaining_weekly);
    if stake > cap {
        let capped = cap.max(0.0);
        let warning = format!(
            "Bankroll cap: proposed ${:.2} exceeds remaining daily ${:.2} / weekly ${:.2}; capped at ${:.2}.",
            stake, summary.remaining_daily, summary.remaining_weekly, capped
        );
        return (capped, Some(warning));
    }
    (stake, None)
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_american_to_decimal() {
        assert!((american_to_decimal(-110.0) - 1.909).abs() < 0.01);
        assert!((american_to_decimal(100.0) - 2.0).abs() < 0.01);
        assert!((american_to_decimal(150.0) - 2.5).abs() < 0.01);
        assert!((american_to_decimal(-150.0) - 1.667).abs() < 0.01);
    }

    #[test]
    fn test_kelly_criterion() {
        // Fair coin at 2.0 odds: Kelly = 0
        assert!((kelly_criterion(0.5, 2.0) - 0.0).abs() < 0.001);

        // 60% win prob at 2.0 odds: Kelly = 0.2 (20% of bankroll)
        assert!((kelly_criterion(0.6, 2.0) - 0.2).abs() < 0.001);

        // 55% win prob at 1.909 odds (-110): Kelly ≈ 0.055
        let kelly = kelly_criterion(0.55, american_to_decimal(-110.0));
        assert!(kelly > 0.0 && kelly < 0.1);

        // No edge: Kelly = 0
        assert!((kelly_criterion(0.5, american_to_decimal(-110.0)) - 0.0).abs() < 0.001);

        // Negative edge: Kelly = 0
        assert!((kelly_criterion(0.4, american_to_decimal(-110.0)) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_expected_value() {
        // 60% win prob at 2.0 odds, $100 stake
        let ev = expected_value(0.6, 2.0, 100.0);
        assert!((ev - 20.0).abs() < 0.01); // $20 expected profit

        // 50% win prob at 2.0 odds, $100 stake
        let ev = expected_value(0.5, 2.0, 100.0);
        assert!((ev - 0.0).abs() < 0.01); // Break even
    }

    #[test]
    fn test_recommend_bet_positive_edge() {
        let config = BankrollConfig::default();
        let rec = recommend_bet(&config, "Patrick Mahomes", "Passing Yards", 285.5, "Over", 62.0, Some(72));

        assert!(rec.recommended_stake > 0.0);
        assert!(rec.kelly_pct > 0.0);
        assert!(rec.expected_value > 0.0);
        assert_eq!(rec.player_name, "Patrick Mahomes");
    }

    #[test]
    fn test_recommend_bet_no_edge() {
        let config = BankrollConfig::default();
        let rec = recommend_bet(&config, "Player", "Stat", 100.0, "Over", 45.0, Some(30));

        assert_eq!(rec.kelly_pct, 0.0);
        assert_eq!(rec.risk_level, "No Edge");
    }

    #[test]
    fn test_recommend_multiple_bets_sorts_by_ev() {
        let config = BankrollConfig::default();
        let picks = vec![
            PickInput {
                player_name: "Low Edge".into(),
                prop_category: "Passing Yards".into(),
                line: 250.0,
                pick_type: "Over".into(),
                win_probability: 55.0,
                confidence_score: Some(60),
            },
            PickInput {
                player_name: "High Edge".into(),
                prop_category: "Rushing Yards".into(),
                line: 75.0,
                pick_type: "Over".into(),
                win_probability: 70.0,
                confidence_score: Some(85),
            },
        ];

        let recs = recommend_multiple_bets(&config, &picks);
        assert!(!recs.is_empty());
        // Highest EV should be first
        if recs.len() >= 2 {
            assert!(recs[0].expected_value >= recs[1].expected_value);
        }
    }

    #[test]
    fn test_parlay_recommendation() {
        let config = BankrollConfig::default();
        let legs = vec![
            PickInput {
                player_name: "Mahomes".into(),
                prop_category: "Passing Yards".into(),
                line: 285.5,
                pick_type: "Over".into(),
                win_probability: 60.0,
                confidence_score: Some(70),
            },
            PickInput {
                player_name: "Kelce".into(),
                prop_category: "Receptions".into(),
                line: 5.5,
                pick_type: "Over".into(),
                win_probability: 58.0,
                confidence_score: Some(65),
            },
        ];

        let parlay = recommend_parlay(&config, &legs, 1.0);
        assert_eq!(parlay.legs.len(), 2);
        assert!(parlay.combined_probability > 0.0);
        assert!(parlay.combined_probability < 100.0);
    }

    #[test]
    fn test_bankroll_persistence() {
        let mut config = BankrollConfig::default();
        config.total_bankroll = 1500.0;
        config.initial_bankroll = 1000.0;

        record_result(&mut config, 50.0, true, Some(-110.0));
        assert!(config.total_bankroll > 1500.0);

        record_result(&mut config, 50.0, false, None);
        assert!(config.total_bankroll < 1500.0 + 50.0);
    }

    #[test]
    fn test_staking_strategy_display() {
        assert_eq!(
            format!("{}", StakingStrategy::Kelly),
            "Kelly Criterion"
        );
        assert_eq!(
            format!("{}", StakingStrategy::FlatBet),
            "Flat Bet"
        );
    }

    #[test]
    fn test_staking_strategy_parse() {
        assert_eq!(
            "kelly".parse::<StakingStrategy>().unwrap(),
            StakingStrategy::Kelly
        );
        assert_eq!(
            "flat".parse::<StakingStrategy>().unwrap(),
            StakingStrategy::FlatBet
        );
        assert!("invalid".parse::<StakingStrategy>().is_err());
    }

    #[test]
    fn test_apply_bankroll_cap() {
        let mut summary = BankrollSummary {
            config: BankrollConfig::default(),
            roi_pct: 0.0,
            total_wagered: 0.0,
            total_won: 0.0,
            total_lost: 0.0,
            profit_loss: 0.0,
            net_profit: 0.0,
            current_bankroll: 1000.0,
            bets_placed: 0.0,
            win_rate: 0.0,
            bets_today: 0.0,
            bets_this_week: 0.0,
            remaining_daily: 75.0,
            remaining_weekly: 200.0,
            daily_limit_used: 125.0,
            weekly_limit_used: 300.0,
            prediction_open_exposure: 0.0,
            paper_open_exposure: 0.0,
            paper_cash_balance: 0.0,
            paper_realized_pnl: 0.0,
            synced_at: String::new(),
        };

        let (capped, warning) = apply_bankroll_cap(100.0, &summary);
        assert!((capped - 75.0).abs() < 0.001);
        assert!(warning.unwrap().contains("Bankroll cap"));

        summary.remaining_daily = 150.0;
        let (capped, warning) = apply_bankroll_cap(100.0, &summary);
        assert!((capped - 100.0).abs() < 0.001);
        assert!(warning.is_none());
    }
}
