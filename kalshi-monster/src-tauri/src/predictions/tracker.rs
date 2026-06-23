use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use sqlx::sqlite::SqlitePoolOptions;
use uuid::Uuid;
use chrono::Datelike;

use super::storage;
use crate::chat::decision_schema::{ContractSide, KalshiTradeDecision};
use crate::kalshi::grading::{infer_market_price_at_entry, resolved_bet_won};
use crate::kalshi::models::{
    KalshiPrediction, KalshiPredictionStats, KalshiGradingSummary, KalshiGradingResult, CategoryStats,
};

/// A single prediction extracted from an AI response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Prediction {
    pub id: String,
    pub session_id: String,
    pub raw_text: String,
    pub player_name: Option<String>,
    pub pick_type: Option<String>,  // "Over" or "Under"
    pub line: Option<f64>,
    pub stat_category: Option<String>,
    pub confidence: Option<String>, // "High", "Medium", "Low"
    pub confidence_score: Option<u8>, // 0-100 numeric score from LLM
    pub probability: Option<f64>,
    pub reasoning: Option<String>,
    pub risk: Option<String>,
    pub created_at: String,
    /// Serialized `KalshiTradeDecision` JSON when the prediction is a Kalshi market trade.
    pub full_decision_json: Option<String>,
    /// Entry price (market implied probability at time of decision, 0-1)
    pub entry_price: Option<f64>,
    /// Whether the model's fair probability diverged significantly from market implied prob
    pub model_disagreement: bool,
}

/// Outcome tracking for a prediction
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum PredictionOutcome {
    Pending,
    Win,
    Loss,
    Push,
    Void,
}

impl std::fmt::Display for PredictionOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PredictionOutcome::Pending => write!(f, "Pending"),
            PredictionOutcome::Win => write!(f, "Win"),
            PredictionOutcome::Loss => write!(f, "Loss"),
            PredictionOutcome::Push => write!(f, "Push"),
            PredictionOutcome::Void => write!(f, "Void"),
        }
    }
}

impl std::str::FromStr for PredictionOutcome {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(PredictionOutcome::Pending),
            "win" => Ok(PredictionOutcome::Win),
            "loss" => Ok(PredictionOutcome::Loss),
            "push" => Ok(PredictionOutcome::Push),
            "void" => Ok(PredictionOutcome::Void),
            _ => Err(format!("Unknown outcome: {}", s)),
        }
    }
}

/// A prediction record with outcome tracking
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PredictionRecord {
    pub prediction: Prediction,
    pub outcome: PredictionOutcome,
    pub actual_result: Option<f64>,
    pub notes: Option<String>,
    pub resolved_at: Option<String>,
}

/// Score range bucket for confidence distribution analysis
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ScoreRange {
    pub range: String,
    pub count: u32,
    pub wins: u32,
    pub losses: u32,
    pub pushes: u32,
    pub pending: u32,
    pub avg_score: f64,
    pub win_rate: f64,
}

/// Calibration metrics for evaluating LLM confidence accuracy
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CalibrationMetrics {
    pub brier_score: f64,
    pub brier_skill_score: f64,
    pub calibration_slope: f64,
    pub calibration_intercept: f64,
}

/// Statistics summary for predictions
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

// ── Trend Analysis Types ──

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrendDataPoint {
    pub week_label: String,
    pub week_start: String,
    pub total: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub avg_confidence: f64,
    pub rolling_avg: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerTrend {
    pub player_name: String,
    pub data_points: Vec<TrendDataPoint>,
    pub overall_win_rate: f64,
    pub total_predictions: u32,
    pub trend_direction: String,
    pub trend_slope: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatCategoryTrend {
    pub stat_category: String,
    pub data_points: Vec<TrendDataPoint>,
    pub overall_win_rate: f64,
    pub total_predictions: u32,
    pub trend_direction: String,
    pub trend_slope: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OverallTrend {
    pub data_points: Vec<TrendDataPoint>,
    pub best_player: Option<String>,
    pub best_player_win_rate: f64,
    pub worst_player: Option<String>,
    pub worst_player_win_rate: f64,
    pub best_stat_category: Option<String>,
    pub best_stat_win_rate: f64,
    pub worst_stat_category: Option<String>,
    pub worst_stat_win_rate: f64,
    pub improving: bool,
    pub overall_trend_slope: f64,
}

/// Manages prediction storage via SQLite
pub struct PredictionTracker {
    pool: Pool<Sqlite>,
}

impl PredictionTracker {
    /// Create a new tracker with an existing DB pool.
    /// Migrates JSON data on first run.
    pub async fn new(pool: Pool<Sqlite>) -> Self {
        // Migrate existing JSON data (safe to call multiple times)
        match storage::migrate_from_json(&pool).await {
            Ok(n) if n > 0 => tracing::info!("Migrated {} predictions from JSON to SQLite", n),
            _ => {}
        }
        PredictionTracker { pool }
    }

    /// Extract predictions from AI response text.
    /// Supports two formats:
    /// 1. JSON blocks (```json ... ```) — structured output from the LLM
    /// 2. Emoji-text format (🏈 PICK: ...) — fallback parsing
    pub fn extract_predictions(&self, session_id: &str, text: &str) -> Vec<Prediction> {
        let mut predictions = Vec::new();
        let now = chrono::Utc::now().to_rfc3339();

        // First, try to extract predictions from JSON blocks
        let json_predictions = self.extract_json_predictions(session_id, text, &now);
        if !json_predictions.is_empty() {
            predictions.extend(json_predictions);
        }

        // Then, also try the emoji-text format as fallback/supplement
        let emoji_predictions = self.extract_emoji_predictions(session_id, text, &now);
        for ep in emoji_predictions {
            let is_dup = predictions.iter().any(|jp: &Prediction| {
                jp.player_name == ep.player_name
                    && jp.stat_category == ep.stat_category
                    && jp.line == ep.line
            });
            if !is_dup {
                predictions.push(ep);
            }
        }

        predictions
    }

    fn extract_json_predictions(&self, session_id: &str, text: &str, now: &str) -> Vec<Prediction> {
        let mut predictions = Vec::new();
        let mut search_start = 0;

        while let Some(block_start) = text[search_start..].find("```json") {
            let abs_start = search_start + block_start + 7;
            if let Some(block_end) = text[abs_start..].find("```") {
                let json_str = &text[abs_start..abs_start + block_end];
                let trimmed = json_str.trim();

                if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    if let Some(pred) = self.parse_kalshi_json_prediction(session_id, &val, now, text) {
                        predictions.push(pred);
                    } else if let Some(pred) = self.parse_json_prediction(session_id, &val, now, text) {
                        predictions.push(pred);
                    }
                }

                if let Ok(val) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed) {
                    for item in &val {
                        if let Some(pred) = self.parse_kalshi_json_prediction(session_id, item, now, text) {
                            predictions.push(pred);
                        } else if let Some(pred) = self.parse_json_prediction(session_id, item, now, text) {
                            predictions.push(pred);
                        }
                    }
                }

                search_start = abs_start + block_end + 3;
            } else {
                break;
            }
        }

        // Also try inline JSON
        if predictions.is_empty() {
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('{') && trimmed.ends_with('}') {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        if let Some(pred) = self.parse_kalshi_json_prediction(session_id, &val, now, text) {
                            predictions.push(pred);
                        } else if let Some(pred) = self.parse_json_prediction(session_id, &val, now, text) {
                            predictions.push(pred);
                        }
                    }
                }
            }
        }

        predictions
    }

    fn parse_json_prediction(
        &self,
        session_id: &str,
        val: &serde_json::Value,
        now: &str,
        raw_text: &str,
    ) -> Option<Prediction> {
        let player_name = val.get("player").and_then(|v| v.as_str())?;
        let pick_type = val.get("pick").and_then(|v| v.as_str())?;

        let pick_normalized = match pick_type.to_lowercase().as_str() {
            "over" | "yes" => "Over".to_string(),
            "under" | "no" => "Under".to_string(),
            _ => return None,
        };

        let confidence_str = val.get("confidence").and_then(|v| v.as_str());
        let confidence_score = val.get("confidence_score").and_then(|v| v.as_u64()).map(|n| n as u8);

        Some(Prediction {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            raw_text: raw_text.to_string(),
            player_name: Some(player_name.to_string()),
            pick_type: Some(pick_normalized),
            line: val.get("line").and_then(|v| v.as_f64()),
            stat_category: val.get("prop_category").and_then(|v| v.as_str()).map(String::from),
            confidence: confidence_str.map(String::from),
            confidence_score,
            probability: val.get("win_probability").and_then(|v| {
                let n = v.as_f64()?;
                if n <= 1.0 { Some(n * 100.0) } else { Some(n) }
            }),
            reasoning: val.get("reasoning").and_then(|v| v.as_str()).map(String::from),
            risk: val.get("key_risk").and_then(|v| v.as_str()).map(String::from)
                .or_else(|| val.get("risk").and_then(|v| v.as_str()).map(String::from))
                .or_else(|| {
                    val.get("risk_factors").and_then(|v| {
                        if v.is_array() {
                            let arr = v.as_array()?;
                            let strs: Vec<&str> = arr.iter().filter_map(|x| x.as_str()).collect();
                            if !strs.is_empty() {
                                Some(strs.join(", "))
                            } else {
                                None
                            }
                        } else {
                            v.as_str().map(String::from)
                        }
                    })
                }),
            created_at: now.to_string(),
            full_decision_json: None,
            entry_price: None,
            model_disagreement: false,
        })
    }

    fn is_kalshi_trade_json(val: &serde_json::Value) -> bool {
        val.get("ticker")
            .and_then(|v| v.as_str())
            .map(|t| t.starts_with("KX") || t.contains('-'))
            .unwrap_or(false)
            && val.get("fair_probability_pct").is_some()
    }

    fn parse_kalshi_json_prediction(
        &self,
        session_id: &str,
        val: &serde_json::Value,
        now: &str,
        raw_text: &str,
    ) -> Option<Prediction> {
        if !Self::is_kalshi_trade_json(val) {
            return None;
        }
        let decision: KalshiTradeDecision = serde_json::from_value(val.clone()).ok()?;
        let full_json = serde_json::to_string(&decision).ok()?;
        let pick_type = match decision.contract_side {
            ContractSide::YES => Some("Over".to_string()),
            ContractSide::NO => Some("Under".to_string()),
            ContractSide::PASS => None,
        };
        let confidence_score = match decision.confidence_tier {
            crate::chat::decision_schema::ConfidenceTier::High => Some(85u8),
            crate::chat::decision_schema::ConfidenceTier::Medium => Some(65u8),
            crate::chat::decision_schema::ConfidenceTier::Low => Some(40u8),
            crate::chat::decision_schema::ConfidenceTier::None => None,
        };
        let risk = if decision.risk_flags.is_empty() {
            None
        } else {
            Some(
                decision
                    .risk_flags
                    .iter()
                    .map(|f| format!("{:?}", f))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        };

        Some(Prediction {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            raw_text: raw_text.to_string(),
            player_name: Some(decision.ticker.clone()),
            pick_type,
            line: if decision.recommended_stake_dollars > 0.0 {
                Some(decision.recommended_stake_dollars)
            } else {
                None
            },
            stat_category: Some(decision.category.clone()),
            confidence: Some(format!("{:?}", decision.confidence_tier)),
            confidence_score,
            probability: Some(decision.fair_probability_pct),
            reasoning: if decision.thesis.is_empty() {
                None
            } else {
                Some(decision.thesis.clone())
            },
            risk,
            created_at: now.to_string(),
            full_decision_json: Some(full_json),
            entry_price: Some(decision.market_price_pct / 100.0),
            model_disagreement: decision.model_disagreement,
        })
    }

    fn extract_emoji_predictions(&self, session_id: &str, text: &str, now: &str) -> Vec<Prediction> {
        let mut predictions = Vec::new();
        let text_normalized = text.replace("👹 PICK:", "🏈 PICK:").replace("\n👹", "\n🏈");
        let sections: Vec<&str> = text_normalized.split("🏈 PICK:").collect();

        for section in sections.iter().skip(1) {
            let end = section.find("\n🏈").unwrap_or(section.len());
            let block = &section[..end];

            let mut prediction = Prediction {
                id: Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                raw_text: format!("🏈 PICK:{}", block),
                player_name: None,
                pick_type: None,
                line: None,
                stat_category: None,
                confidence: None,
                confidence_score: None,
                probability: None,
                reasoning: None,
                risk: None,
                created_at: now.to_string(),
                full_decision_json: None,
                entry_price: None,
                model_disagreement: false,
            };

            let first_line = block.lines().next().unwrap_or("").trim();
            if let Some(rest) = first_line.strip_prefix("Under ") {
                prediction.pick_type = Some("Under".to_string());
                Self::parse_pick_details(rest, &mut prediction);
            } else if let Some(rest) = first_line.strip_prefix("Over ") {
                prediction.pick_type = Some("Over".to_string());
                Self::parse_pick_details(rest, &mut prediction);
            } else if let Some(rest) = first_line.strip_prefix("NO ") {
                prediction.pick_type = Some("Under".to_string());
                Self::parse_pick_details(rest, &mut prediction);
            } else if let Some(rest) = first_line.strip_prefix("YES ") {
                prediction.pick_type = Some("Over".to_string());
                Self::parse_pick_details(rest, &mut prediction);
            }

            for line in block.lines() {
                let trimmed = line.trim();
                if let Some(conf) = trimmed.strip_prefix("⚡ CONFIDENCE:") {
                    prediction.confidence = Some(conf.trim().to_string());
                } else if let Some(prob) = trimmed.strip_prefix("📈 PROBABILITY:") {
                    let prob_str = prob.trim();
                    if let Some(pct_end) = prob_str.find('%') {
                        if let Ok(pct) = prob_str[..pct_end].trim().parse::<f64>() {
                            prediction.probability = Some(pct);
                        }
                    }
                } else if let Some(reason) = trimmed.strip_prefix("📊 REASONING:") {
                    prediction.reasoning = Some(reason.trim().to_string());
                } else if let Some(risk) = trimmed.strip_prefix("⚠️ RISK:") {
                    prediction.risk = Some(risk.trim().to_string());
                }
            }

            if prediction.pick_type.is_some() {
                predictions.push(prediction);
            }
        }

        predictions
    }

    fn parse_pick_details(rest: &str, prediction: &mut Prediction) {
        let parts: Vec<&str> = rest.split(" for ").collect();
        if parts.len() >= 2 {
            if let Ok(line_val) = parts[0].trim().parse::<f64>() {
                prediction.line = Some(line_val);
            } else {
                let clean_price: String = parts[0].chars().filter(|c| c.is_digit(10) || *c == '.').collect();
                if let Ok(val) = clean_price.parse::<f64>() {
                    if parts[0].contains('¢') {
                        prediction.line = Some(val / 100.0);
                    } else {
                        prediction.line = Some(val);
                    }
                }
            }

            let name_and_stat = parts[1..].join(" for ");
            if let Some(dash_pos) = name_and_stat.find('—') {
                prediction.player_name = Some(name_and_stat[..dash_pos].trim().to_string());
                prediction.stat_category = Some(name_and_stat[dash_pos + 3..].trim().to_string());
            } else if let Some(dash_pos) = name_and_stat.rfind(" - ") {
                prediction.player_name = Some(name_and_stat[..dash_pos].trim().to_string());
                prediction.stat_category = Some(name_and_stat[dash_pos + 3..].trim().to_string());
            } else {
                prediction.player_name = Some(name_and_stat.trim().to_string());
            }
        }
    }

    /// Save a prediction record to SQLite
    pub async fn save_prediction(&self, record: PredictionRecord) -> Result<(), String> {
        storage::insert_prediction(&self.pool, &record).await
    }

    /// Update prediction outcome in SQLite
    pub async fn update_outcome(
        &self,
        prediction_id: &str,
        outcome: PredictionOutcome,
        actual_result: Option<f64>,
    ) -> Result<(), String> {
        storage::update_prediction_outcome(&self.pool, prediction_id, &outcome, actual_result).await
    }

    /// Get all predictions for a session
    pub async fn get_session_predictions(&self, session_id: &str) -> Vec<PredictionRecord> {
        storage::get_session_predictions(&self.pool, session_id)
            .await
            .unwrap_or_default()
    }

    /// Get all predictions across all sessions
    pub async fn get_all_predictions(&self) -> Vec<PredictionRecord> {
        storage::get_all_predictions(&self.pool).await.unwrap_or_default()
    }

    /// Calculate prediction statistics
    pub fn get_stats(&self, records: &[PredictionRecord]) -> PredictionStats {
        let all = records;
        let total = all.len() as u32;
        let wins = all.iter().filter(|r| r.outcome == PredictionOutcome::Win).count() as u32;
        let losses = all.iter().filter(|r| r.outcome == PredictionOutcome::Loss).count() as u32;
        let pushes = all.iter().filter(|r| r.outcome == PredictionOutcome::Push).count() as u32;
        let pending = all.iter().filter(|r| r.outcome == PredictionOutcome::Pending).count() as u32;

        let decided = (wins + losses) as f64;
        let win_rate = if decided > 0.0 { wins as f64 / decided * 100.0 } else { 0.0 };

        let scored: Vec<u8> = all.iter().filter_map(|r| r.prediction.confidence_score).collect();
        let avg_confidence_score = if !scored.is_empty() {
            scored.iter().map(|&s| s as f64).sum::<f64>() / scored.len() as f64
        } else {
            0.0
        };

        let high_conf: Vec<&PredictionRecord> = all
            .iter()
            .filter(|r| r.prediction.confidence.as_deref() == Some("High") && r.outcome != PredictionOutcome::Pending)
            .collect();
        let med_conf: Vec<&PredictionRecord> = all
            .iter()
            .filter(|r| r.prediction.confidence.as_deref() == Some("Medium") && r.outcome != PredictionOutcome::Pending)
            .collect();
        let low_conf: Vec<&PredictionRecord> = all
            .iter()
            .filter(|r| r.prediction.confidence.as_deref() == Some("Low") && r.outcome != PredictionOutcome::Pending)
            .collect();

        // Calibration via the shared edge-eval engine. Scores the stored
        // win probability (NOT confidence_score, which is a separate 0-100
        // self-assessment) and excludes pushes/voids, which carry no
        // accuracy signal. Fixes the original implementation that graded
        // confidence_score as if it were a probability and counted pushes
        // as losses.
        let graded = crate::eval_adapter::records_to_graded(all);
        let cal = edge_eval::calibration::calibration_metrics(&graded, 10, 0, 42);
        let (brier_score, brier_skill_score) = if cal.n == 0 {
            (0.0, 0.0)
        } else {
            (
                cal.brier_score,
                if cal.brier_skill_score.is_finite() {
                    cal.brier_skill_score
                } else {
                    0.0
                },
            )
        };

        let (calibration_slope, calibration_intercept) = {
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut sum_xy = 0.0;
            let mut sum_x2 = 0.0;
            let mut n = 0.0;
            for bin in cal.reliability.iter().filter(|b| b.n > 0) {
                let x = bin.mean_predicted;
                let y = bin.empirical;
                sum_x += x;
                sum_y += y;
                sum_xy += x * y;
                sum_x2 += x * x;
                n += 1.0;
            }
            if n > 1.0 {
                let denom = n * sum_x2 - sum_x * sum_x;
                if denom.abs() > f64::EPSILON {
                    let slope = (n * sum_xy - sum_x * sum_y) / denom;
                    let intercept = (sum_y - slope * sum_x) / n;
                    (slope, intercept)
                } else {
                    (0.0, 0.0)
                }
            } else {
                (0.0, 0.0)
            }
        };

        let ranges = vec![(80, 100), (60, 79), (40, 59), (0, 39)];
        let range_names = vec!["80-100", "60-79", "40-59", "0-39"];
        let mut score_distribution = Vec::new();

        for (i, (lo, hi)) in ranges.iter().enumerate() {
            let bucket: Vec<&PredictionRecord> = all
                .iter()
                .filter(|r| {
                    r.prediction.confidence_score.map_or(false, |s| s >= *lo && s <= *hi)
                })
                .collect();
            let count = bucket.len() as u32;
            let wins = bucket.iter().filter(|r| r.outcome == PredictionOutcome::Win).count() as u32;
            let losses = bucket.iter().filter(|r| r.outcome == PredictionOutcome::Loss).count() as u32;
            let pushes = bucket.iter().filter(|r| r.outcome == PredictionOutcome::Push).count() as u32;
            let pending = bucket.iter().filter(|r| r.outcome == PredictionOutcome::Pending).count() as u32;
            let avg_score = if count > 0 {
                bucket.iter().filter_map(|r| r.prediction.confidence_score.map(|s| s as f64)).sum::<f64>() / count as f64
            } else {
                0.0
            };
            let decided = (wins + losses) as f64;
            let win_rate = if decided > 0.0 { wins as f64 / decided * 100.0 } else { 0.0 };

            score_distribution.push(ScoreRange {
                range: range_names[i].to_string(),
                count,
                wins,
                losses,
                pushes,
                pending,
                avg_score,
                win_rate,
            });
        }

        PredictionStats {
            total,
            wins,
            losses,
            pushes,
            pending,
            win_rate,
            avg_confidence_score,
            high_confidence_wins: high_conf.iter().filter(|r| r.outcome == PredictionOutcome::Win).count() as u32,
            high_confidence_total: high_conf.len() as u32,
            medium_confidence_wins: med_conf.iter().filter(|r| r.outcome == PredictionOutcome::Win).count() as u32,
            medium_confidence_total: med_conf.len() as u32,
            low_confidence_wins: low_conf.iter().filter(|r| r.outcome == PredictionOutcome::Win).count() as u32,
            low_confidence_total: low_conf.len() as u32,
            calibration: CalibrationMetrics {
                brier_score: (brier_score * 1000.0).round() / 1000.0,
                brier_skill_score: (brier_skill_score * 1000.0).round() / 1000.0,
                calibration_slope: (calibration_slope * 1000.0).round() / 1000.0,
                calibration_intercept: (calibration_intercept * 1000.0).round() / 1000.0,
            },
            score_distribution,
        }
    }

    // ── Trend Analysis ──

    fn compute_trend_data(records: &[&PredictionRecord]) -> Vec<TrendDataPoint> {
        if records.is_empty() {
            return Vec::new();
        }

        let mut buckets: std::collections::HashMap<String, Vec<&PredictionRecord>> = std::collections::HashMap::new();
        for r in records {
            if r.outcome == PredictionOutcome::Pending {
                continue;
            }
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&r.prediction.created_at) {
                let iso_week = dt.iso_week();
                let week_key = format!("{}-W{:02}", iso_week.year(), iso_week.week());
                buckets.entry(week_key).or_default().push(r);
            }
        }

        if buckets.is_empty() {
            return Vec::new();
        }

        let mut weeks: Vec<String> = buckets.keys().cloned().collect();
        weeks.sort();

        let mut data_points = Vec::new();
        let mut cumulative_wins = 0u32;
        let mut cumulative_total = 0u32;

        for week in &weeks {
            let bucket = buckets.get(week).unwrap();
            let total = bucket.len() as u32;
            let wins = bucket.iter().filter(|r| r.outcome == PredictionOutcome::Win).count() as u32;
            let losses = bucket.iter().filter(|r| r.outcome == PredictionOutcome::Loss).count() as u32;
            let decided = (wins + losses) as f64;
            let win_rate = if decided > 0.0 { wins as f64 / decided * 100.0 } else { 0.0 };

            let scored: Vec<u8> = bucket.iter().filter_map(|r| r.prediction.confidence_score).collect();
            let avg_confidence = if !scored.is_empty() {
                scored.iter().map(|&s| s as f64).sum::<f64>() / scored.len() as f64
            } else {
                0.0
            };

            cumulative_wins += wins;
            cumulative_total += wins + losses;
            let rolling_avg = if cumulative_total > 0 {
                cumulative_wins as f64 / cumulative_total as f64 * 100.0
            } else {
                0.0
            };

            let week_start = bucket
                .first()
                .and_then(|r| chrono::DateTime::parse_from_rfc3339(&r.prediction.created_at).ok())
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();

            data_points.push(TrendDataPoint {
                week_label: week.clone(),
                week_start,
                total,
                wins,
                losses,
                win_rate: (win_rate * 10.0).round() / 10.0,
                avg_confidence: (avg_confidence * 10.0).round() / 10.0,
                rolling_avg: (rolling_avg * 10.0).round() / 10.0,
            });
        }

        data_points
    }

    fn compute_trend_slope(data_points: &[TrendDataPoint]) -> f64 {
        if data_points.len() < 2 {
            return 0.0;
        }
        let n = data_points.len() as f64;
        let sum_x: f64 = (0..data_points.len()).map(|i| i as f64).sum();
        let sum_y: f64 = data_points.iter().map(|dp| dp.win_rate).sum();
        let sum_xy: f64 = data_points.iter().enumerate().map(|(i, dp)| i as f64 * dp.win_rate).sum();
        let sum_x2: f64 = (0..data_points.len()).map(|i| (i as f64).powi(2)).sum();

        let denom = n * sum_x2 - sum_x * sum_x;
        if denom.abs() < f64::EPSILON {
            return 0.0;
        }
        let slope = (n * sum_xy - sum_x * sum_y) / denom;
        (slope * 100.0).round() / 100.0
    }

    fn trend_direction(slope: f64) -> String {
        if slope > 1.0 {
            "improving".to_string()
        } else if slope < -1.0 {
            "declining".to_string()
        } else {
            "stable".to_string()
        }
    }

    /// Get trend data for a specific player.
    pub async fn get_player_trend(&self, player_name: &str) -> Option<PlayerTrend> {
        let all = self.get_all_predictions().await;
        let player_records: Vec<&PredictionRecord> = all
            .iter()
            .filter(|r| {
                r.prediction
                    .player_name
                    .as_deref()
                    .map(|n| n.eq_ignore_ascii_case(player_name))
                    .unwrap_or(false)
            })
            .collect();

        if player_records.is_empty() {
            return None;
        }

        let data_points = Self::compute_trend_data(&player_records);
        let total_predictions = player_records.len() as u32;

        let wins = player_records.iter().filter(|r| r.outcome == PredictionOutcome::Win).count() as u32;
        let losses = player_records.iter().filter(|r| r.outcome == PredictionOutcome::Loss).count() as u32;
        let decided = (wins + losses) as f64;
        let overall_win_rate = if decided > 0.0 { wins as f64 / decided * 100.0 } else { 0.0 };

        let slope = Self::compute_trend_slope(&data_points);

        Some(PlayerTrend {
            player_name: player_name.to_string(),
            data_points,
            overall_win_rate: (overall_win_rate * 10.0).round() / 10.0,
            total_predictions,
            trend_direction: Self::trend_direction(slope),
            trend_slope: slope,
        })
    }

    /// Get trend data for a specific stat category.
    pub async fn get_stat_category_trend(&self, stat_category: &str) -> Option<StatCategoryTrend> {
        let all = self.get_all_predictions().await;
        let cat_records: Vec<&PredictionRecord> = all
            .iter()
            .filter(|r| {
                r.prediction
                    .stat_category
                    .as_deref()
                    .map(|c| c.eq_ignore_ascii_case(stat_category))
                    .unwrap_or(false)
            })
            .collect();

        if cat_records.is_empty() {
            return None;
        }

        let data_points = Self::compute_trend_data(&cat_records);
        let total_predictions = cat_records.len() as u32;

        let wins = cat_records.iter().filter(|r| r.outcome == PredictionOutcome::Win).count() as u32;
        let losses = cat_records.iter().filter(|r| r.outcome == PredictionOutcome::Loss).count() as u32;
        let decided = (wins + losses) as f64;
        let overall_win_rate = if decided > 0.0 { wins as f64 / decided * 100.0 } else { 0.0 };

        let slope = Self::compute_trend_slope(&data_points);

        Some(StatCategoryTrend {
            stat_category: stat_category.to_string(),
            data_points,
            overall_win_rate: (overall_win_rate * 10.0).round() / 10.0,
            total_predictions,
            trend_direction: Self::trend_direction(slope),
            trend_slope: slope,
        })
    }

    /// Get overall AI performance trend.
    pub async fn get_overall_trend(&self) -> OverallTrend {
        let all = self.get_all_predictions().await;
        let resolved: Vec<&PredictionRecord> = all
            .iter()
            .filter(|r| r.outcome != PredictionOutcome::Pending)
            .collect();

        let data_points = Self::compute_trend_data(&resolved);
        let slope = Self::compute_trend_slope(&data_points);

        let mut player_map: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new();
        for r in &resolved {
            if let Some(ref name) = r.prediction.player_name {
                let entry = player_map.entry(name.clone()).or_insert((0, 0));
                entry.1 += 1;
                if r.outcome == PredictionOutcome::Win {
                    entry.0 += 1;
                }
            }
        }

        let mut best_player: Option<String> = None;
        let mut best_player_wr = 0.0f64;
        let mut worst_player: Option<String> = None;
        let mut worst_player_wr = 100.0f64;

        for (name, (wins, total)) in &player_map {
            if *total < 3 { continue; }
            let wr = *wins as f64 / *total as f64 * 100.0;
            if wr > best_player_wr {
                best_player_wr = wr;
                best_player = Some(name.clone());
            }
            if wr < worst_player_wr {
                worst_player_wr = wr;
                worst_player = Some(name.clone());
            }
        }

        let mut stat_map: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new();
        for r in &resolved {
            if let Some(ref cat) = r.prediction.stat_category {
                let entry = stat_map.entry(cat.clone()).or_insert((0, 0));
                entry.1 += 1;
                if r.outcome == PredictionOutcome::Win {
                    entry.0 += 1;
                }
            }
        }

        let mut best_stat: Option<String> = None;
        let mut best_stat_wr = 0.0f64;
        let mut worst_stat: Option<String> = None;
        let mut worst_stat_wr = 100.0f64;

        for (cat, (wins, total)) in &stat_map {
            if *total < 3 { continue; }
            let wr = *wins as f64 / *total as f64 * 100.0;
            if wr > best_stat_wr {
                best_stat_wr = wr;
                best_stat = Some(cat.clone());
            }
            if wr < worst_stat_wr {
                worst_stat_wr = wr;
                worst_stat = Some(cat.clone());
            }
        }

        OverallTrend {
            data_points,
            best_player,
            best_player_win_rate: (best_player_wr * 10.0).round() / 10.0,
            worst_player,
            worst_player_win_rate: (worst_player_wr * 10.0).round() / 10.0,
            best_stat_category: best_stat,
            best_stat_win_rate: (best_stat_wr * 10.0).round() / 10.0,
            worst_stat_category: worst_stat,
            worst_stat_win_rate: (worst_stat_wr * 10.0).round() / 10.0,
            improving: slope > 1.0,
            overall_trend_slope: slope,
        }
    }

    /// Get all players that have at least one prediction, sorted by total predictions desc.
    pub async fn get_player_list(&self) -> Vec<(String, u32)> {
        let all = self.get_all_predictions().await;
        let mut map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for r in &all {
            if let Some(ref name) = r.prediction.player_name {
                *map.entry(name.clone()).or_insert(0) += 1;
            }
        }
        let mut list: Vec<(String, u32)> = map.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1));
        list
    }

    /// Get all stat categories that have at least one prediction, sorted by total desc.
    pub async fn get_stat_category_list(&self) -> Vec<(String, u32)> {
        let all = self.get_all_predictions().await;
        let mut map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for r in &all {
            if let Some(ref cat) = r.prediction.stat_category {
                *map.entry(cat.clone()).or_insert(0) += 1;
            }
        }
        let mut list: Vec<(String, u32)> = map.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1));
        list
    }

    fn extract_ticker_from_text(text: &str) -> Option<String> {
        if let Some(pos) = text.find("Ticker") {
            let sub = &text[pos..];
            let lines: Vec<&str> = sub.lines().collect();
            if let Some(first_line) = lines.first() {
                let clean = first_line.replace("**", "").replace(":", "");
                let parts: Vec<&str> = clean.split_whitespace().collect();
                if parts.len() >= 2 {
                    return Some(parts[1].trim().to_string());
                }
            }
        }
        let re = regex::Regex::new(r"KX[A-Z0-9\-]+").ok()?;
        re.find(text).map(|m| m.as_str().to_string())
    }

    fn parse_kalshi_decision_blob(json: &str) -> Option<KalshiTradeDecision> {
        serde_json::from_str::<KalshiTradeDecision>(json).ok()
    }

    fn kalshi_decision_from_record(r: &PredictionRecord) -> Option<KalshiTradeDecision> {
        if let Some(ref blob) = r.prediction.full_decision_json {
            if let Some(d) = Self::parse_kalshi_decision_blob(blob) {
                return Some(d);
            }
        }
        if let Some(ref notes) = r.notes {
            if notes.trim_start().starts_with('{') {
                if let Some(d) = Self::parse_kalshi_decision_blob(notes) {
                    return Some(d);
                }
            }
        }
        Self::find_kalshi_decision_in_text(&r.prediction.raw_text)
    }

    fn find_kalshi_decision_in_text(text: &str) -> Option<KalshiTradeDecision> {
        let mut search_start = 0;
        while let Some(block_start) = text[search_start..].find("```json") {
            let abs_start = search_start + block_start + 7;
            if let Some(block_end) = text[abs_start..].find("```") {
                let json_str = text[abs_start..abs_start + block_end].trim();
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                    if Self::is_kalshi_trade_json(&val) {
                        if let Ok(d) = serde_json::from_value(val) {
                            return Some(d);
                        }
                    }
                }
                search_start = abs_start + block_end + 3;
            } else {
                break;
            }
        }
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    if Self::is_kalshi_trade_json(&val) {
                        if let Ok(d) = serde_json::from_value(val) {
                            return Some(d);
                        }
                    }
                }
            }
        }
        None
    }

    fn record_is_kalshi(r: &PredictionRecord) -> bool {
        r.prediction.full_decision_json.is_some()
            || Self::kalshi_decision_from_record(r).is_some()
            || r.prediction
                .player_name
                .as_deref()
                .map_or(false, |n| n.starts_with("KX") || n.contains('-'))
            || r.prediction.raw_text.to_lowercase().contains("kalshi")
    }

    fn kalshi_prediction_from_record(r: &PredictionRecord) -> Option<KalshiPrediction> {
        if !Self::record_is_kalshi(r) {
            return None;
        }

        let decision = Self::kalshi_decision_from_record(r);
        let ticker = decision
            .as_ref()
            .map(|d| d.ticker.clone())
            .or_else(|| {
                r.prediction
                    .player_name
                    .as_deref()
                    .filter(|n| n.starts_with("KX"))
                    .map(|s| s.to_string())
            })
            .or_else(|| Self::extract_ticker_from_text(&r.prediction.raw_text))
            .unwrap_or_default();

        if ticker.is_empty() {
            return None;
        }

        let contract_side = decision
            .as_ref()
            .map(|d| format!("{:?}", d.contract_side))
            .or_else(|| {
                r.prediction.pick_type.as_ref().map(|pt| match pt.as_str() {
                    "Over" => "YES".to_string(),
                    "Under" => "NO".to_string(),
                    other => other.to_uppercase(),
                })
            });

        let actual_outcome = if r.outcome == PredictionOutcome::Pending {
            None
        } else if let Some(ref notes) = r.notes {
            if notes.contains("Outcome: Yes") {
                Some("Yes".to_string())
            } else if notes.contains("Outcome: No") {
                Some("No".to_string())
            } else {
                Some(r.outcome.to_string())
            }
        } else {
            Some(r.outcome.to_string())
        };

        let (
            title,
            category,
            fair_prob,
            stake,
            price_to_enter,
            market_price_at_entry,
            edge_points,
            fractional_kelly,
            recommended_stake,
            risk_flags,
            thesis_text,
            data_quality,
            decision_action,
        ) = if let Some(ref d) = decision {
            (
                d.market_title.clone(),
                d.category.clone(),
                d.fair_probability_pct,
                if d.recommended_stake_dollars > 0.0 {
                    d.recommended_stake_dollars
                } else {
                    r.prediction.line.unwrap_or(10.0)
                },
                Some(d.price_to_enter),
                Some(d.market_price_pct),
                Some(d.edge_points),
                Some(d.fractional_kelly_pct),
                Some(d.recommended_stake_dollars),
                Some(
                    d.risk_flags
                        .iter()
                        .map(|f| format!("{:?}", f))
                        .collect(),
                ),
                if d.thesis.is_empty() {
                    r.prediction.reasoning.clone()
                } else {
                    Some(d.thesis.clone())
                },
                Some(format!("{:?}", d.data_quality)),
                Some(format!("{:?}", d.decision)),
            )
        } else {
            (
                r.notes
                    .clone()
                    .unwrap_or_else(|| r.prediction.player_name.clone().unwrap_or_default()),
                r.prediction
                    .stat_category
                    .clone()
                    .unwrap_or_else(|| "Other".to_string()),
                r.prediction.probability.unwrap_or(0.0),
                r.prediction.line.unwrap_or(10.0),
                None,
                None,
                None,
                None,
                None,
                None,
                r.prediction.reasoning.clone(),
                None,
                None,
            )
        };

        let market_price_at_entry = infer_market_price_at_entry(
            market_price_at_entry,
            price_to_enter,
            contract_side.as_deref(),
        );

        Some(KalshiPrediction {
            id: r.prediction.id.clone(),
            ticker,
            title,
            category,
            predicted_probability: fair_prob,
            actual_outcome,
            confidence_score: r.prediction.confidence_score,
            reasoning: thesis_text.clone(),
            created_at: r.prediction.created_at.clone(),
            resolved_at: r.resolved_at.clone(),
            stake_amount: stake,
            pnl: r.actual_result,
            pick_type: r.prediction.pick_type.clone(),
            price_to_enter,
            market_price_at_entry,
            contract_side,
            edge_points,
            fractional_kelly_pct: fractional_kelly,
            recommended_stake_dollars: recommended_stake,
            risk_flags,
            thesis: thesis_text,
            data_quality,
            decision: decision_action,
        })
    }

    /// Get the database pool (for direct access from commands if needed).
    pub async fn get_kalshi_predictions(&self) -> Vec<KalshiPrediction> {
        let all = self.get_all_predictions().await;
        all.iter()
            .filter_map(Self::kalshi_prediction_from_record)
            .collect()
    }

    pub async fn update_kalshi_outcome(
        &self,
        prediction_id: &str,
        actual_outcome: &str,
        pnl: f64,
    ) -> Result<(), String> {
        let outcome = if pnl > 0.0 {
            PredictionOutcome::Win
        } else if pnl < 0.0 {
            PredictionOutcome::Loss
        } else {
            PredictionOutcome::Push
        };

        let notes = format!("Outcome: {}, PnL: {}", actual_outcome, pnl);
        let resolved_at = chrono::Utc::now().to_rfc3339();

        let rows = sqlx::query(
            r#"
            UPDATE predictions
            SET outcome = ?1, actual_result = ?2, notes = ?3, resolved_at = ?4
            WHERE id = ?5
            "#
        )
        .bind(outcome.to_string())
        .bind(pnl)
        .bind(notes)
        .bind(resolved_at)
        .bind(prediction_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to update Kalshi outcome: {}", e))?
        .rows_affected();

        if rows == 0 {
            Err(format!("Prediction {} not found", prediction_id))
        } else {
            Ok(())
        }
    }

    /// Update CLV (Closing Line Value) for a prediction.
    pub async fn update_prediction_clv(
        &self,
        prediction_id: &str,
        close_price: f64,
    ) -> Result<(), String> {
        crate::predictions::storage::update_prediction_clv(&self.pool, prediction_id, close_price).await
    }

    /// Set the entry price for a prediction.
    pub async fn set_prediction_entry_price(
        &self,
        prediction_id: &str,
        entry_price: f64,
    ) -> Result<(), String> {
        crate::predictions::storage::set_prediction_entry_price(&self.pool, prediction_id, entry_price).await
    }

    /// Set model disagreement flag for a prediction.
    pub async fn set_model_disagreement(
        &self,
        prediction_id: &str,
        disagreement: bool,
    ) -> Result<(), String> {
        crate::predictions::storage::set_model_disagreement(&self.pool, prediction_id, disagreement).await
    }

    pub async fn get_kalshi_stats(&self, predictions: &[KalshiPrediction]) -> KalshiPredictionStats {
        let total = predictions.len() as u32;
        let wins = predictions
            .iter()
            .filter(|p| resolved_bet_won(p) == Some(true))
            .count() as u32;
        let losses = predictions
            .iter()
            .filter(|p| resolved_bet_won(p) == Some(false))
            .count() as u32;
        let pending = predictions
            .iter()
            .filter(|p| p.actual_outcome.is_none())
            .count() as u32;

        let decided = (wins + losses) as f64;
        let win_rate = if decided > 0.0 { wins as f64 / decided * 100.0 } else { 0.0 };

        let scored: Vec<u8> = predictions.iter().filter_map(|p| p.confidence_score).collect();
        let avg_confidence_score = if !scored.is_empty() {
            scored.iter().map(|&s| s as f64).sum::<f64>() / scored.len() as f64
        } else {
            0.0
        };

        let total_volume_traded = predictions.iter().map(|p| p.stake_amount).sum::<f64>();
        let total_pnl = predictions.iter().filter_map(|p| p.pnl).sum::<f64>();
        let roi_pct = if total_volume_traded > 0.0 { total_pnl / total_volume_traded * 100.0 } else { 0.0 };

        let resolved: Vec<&KalshiPrediction> = predictions.iter().filter(|p| p.actual_outcome.is_some()).collect();
        let brier_score = if resolved.is_empty() {
            0.0
        } else {
            let sum_sq_error: f64 = resolved.iter().map(|p| {
                let actual = if p.actual_outcome.as_deref() == Some("Yes") { 1.0 } else { 0.0 };
                let diff = actual - (p.predicted_probability / 100.0);
                diff * diff
            }).sum();
            sum_sq_error / resolved.len() as f64
        };

        let mut category_breakdown = std::collections::HashMap::new();
        for p in predictions {
            let entry = category_breakdown.entry(p.category.clone()).or_insert(CategoryStats {
                count: 0,
                wins: 0,
                losses: 0,
                win_rate: 0.0,
            });
            entry.count += 1;
            if p.actual_outcome.is_some() {
                if resolved_bet_won(p) == Some(true) {
                    entry.wins += 1;
                } else if resolved_bet_won(p) == Some(false) {
                    entry.losses += 1;
                }
                let dec = (entry.wins + entry.losses) as f64;
                entry.win_rate = if dec > 0.0 { entry.wins as f64 / dec * 100.0 } else { 0.0 };
            }
        }

        KalshiPredictionStats {
            total,
            wins,
            losses,
            pending,
            win_rate,
            avg_confidence_score,
            total_volume_traded,
            total_pnl,
            roi_pct,
            calibration: crate::kalshi::models::CalibrationMetrics {
                brier_score,
                brier_skill_score: 0.0,
                calibration_slope: 1.0,
                calibration_intercept: 0.0,
            },
            category_breakdown,
        }
    }

    pub async fn get_kalshi_grading_summary(&self) -> KalshiGradingSummary {
        let preds = self.get_kalshi_predictions().await;
        let resolved: Vec<KalshiPrediction> = preds.into_iter().filter(|p| p.actual_outcome.is_some()).collect();
        let total = resolved.len() as u32;
        let mut wins = 0;
        let mut losses = 0;
        let mut total_pnl = 0.0;
        let mut results = Vec::new();

        for p in &resolved {
            let won = resolved_bet_won(p).unwrap_or(false);
            if won {
                wins += 1;
            } else {
                losses += 1;
            }
            let pnl = p.pnl.unwrap_or(0.0);
            total_pnl += pnl;

            results.push(KalshiGradingResult {
                prediction_id: p.id.clone(),
                ticker: p.ticker.clone(),
                title: p.title.clone(),
                category: p.category.clone(),
                predicted_probability: p.predicted_probability,
                actual_outcome: p.actual_outcome.clone().unwrap_or_default(),
                outcome: if won { "Win".to_string() } else { "Loss".to_string() },
                pnl,
                stake_amount: p.stake_amount,
                contract_side: p.contract_side.clone(),
                market_price_at_entry: p.market_price_at_entry,
                notes: None,
                resolved_at: p.resolved_at.clone().unwrap_or_default(),
            });
        }

        KalshiGradingSummary {
            total_predictions: total,
            pending_gradable: 0,
            graded: total,
            wins,
            losses,
            total_pnl,
            results,
            fetched_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
}

// ── Tests ──
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_prediction() {
        // We can't easily create a PredictionTracker without a DB pool in tests,
        // so we test the extraction logic via the static methods.
        // The extraction functions don't actually need the pool.
        let tracker = PredictionTracker__extraction_only();
        let text = r#"```json
{
  "player": "Patrick Mahomes",
  "team": "KC",
  "opponent": "BUF",
  "prop_category": "Passing Yards",
  "line": 285.5,
  "projection": 302.1,
  "pick": "Over",
  "confidence": "High",
  "confidence_score": 72,
  "win_probability": 0.62,
  "reasoning": "Mahomes averages 290 yards/game. BUF ranks 22nd in pass defense.",
  "key_risk": "BUF could lead early, reducing pass volume"
}
```"#;

        let preds = tracker.extract_predictions("test-session", text);
        assert_eq!(preds.len(), 1);
        let p = &preds[0];
        assert_eq!(p.player_name, Some("Patrick Mahomes".to_string()));
        assert_eq!(p.pick_type, Some("Over".to_string()));
        assert_eq!(p.line, Some(285.5));
        assert_eq!(p.stat_category, Some("Passing Yards".to_string()));
        assert_eq!(p.confidence, Some("High".to_string()));
        assert_eq!(p.probability, Some(62.0));
        assert!(p.reasoning.as_ref().unwrap().contains("290 yards"));
        assert!(p.risk.as_ref().unwrap().contains("BUF could lead"));
    }

    #[test]
    fn test_extract_json_array_predictions() {
        let tracker = PredictionTracker__extraction_only();
        let text = r#"```json
[
  {"player": "Josh Allen", "pick": "Over", "line": 272.5, "prop_category": "Passing Yards", "confidence": "Medium", "win_probability": 58, "reasoning": "Allen is elite", "key_risk": "Wind"},
  {"player": "Saquon Barkley", "pick": "Under", "line": 88.5, "prop_category": "Rushing Yards", "confidence": "Low", "win_probability": 45, "reasoning": "Tough matchup", "key_risk": "Game script"}
]
```"#;

        let preds = tracker.extract_predictions("test-session", text);
        assert_eq!(preds.len(), 2);
        assert_eq!(preds[0].player_name, Some("Josh Allen".to_string()));
        assert_eq!(preds[0].pick_type, Some("Over".to_string()));
        assert_eq!(preds[1].player_name, Some("Saquon Barkley".to_string()));
        assert_eq!(preds[1].pick_type, Some("Under".to_string()));
    }

    #[test]
    fn test_extract_emoji_prediction() {
        let tracker = PredictionTracker__extraction_only();
        let text = "🏈 PICK: Over 285.5 for Patrick Mahomes — Passing Yards
📊 REASONING: Mahomes averages 290 yards/game. BUF ranks 22nd in pass defense.
⚡ CONFIDENCE: High
📈 PROBABILITY: 62% Over
⚠️ RISK: BUF could lead early";

        let preds = tracker.extract_predictions("test-session", text);
        assert_eq!(preds.len(), 1);
        let p = &preds[0];
        assert_eq!(p.player_name, Some("Patrick Mahomes".to_string()));
        assert_eq!(p.pick_type, Some("Over".to_string()));
        assert_eq!(p.line, Some(285.5));
        assert_eq!(p.stat_category, Some("Passing Yards".to_string()));
        assert_eq!(p.confidence, Some("High".to_string()));
        assert_eq!(p.probability, Some(62.0));
    }

    #[test]
    fn test_no_duplicate_predictions() {
        let tracker = PredictionTracker__extraction_only();
        let text = r#"```json
{"player": "Patrick Mahomes", "pick": "Over", "line": 285.5, "prop_category": "Passing Yards", "confidence": "High", "win_probability": 62, "reasoning": "Elite", "key_risk": "Wind"}
```

🏈 PICK: Over 285.5 for Patrick Mahomes — Passing Yards
⚡ CONFIDENCE: High
📈 PROBABILITY: 62% Over
⚠️ RISK: Wind"#;

        let preds = tracker.extract_predictions("test-session", text);
        assert_eq!(preds.len(), 1);
    }

    #[test]
    fn test_json_probability_formats() {
        let tracker = PredictionTracker__extraction_only();

        let text1 = r#"```json
{"player": "Test1", "pick": "Over", "line": 100.0, "prop_category": "Yards", "win_probability": 0.62}
```"#;
        let preds1 = tracker.extract_predictions("s", text1);
        assert_eq!(preds1[0].probability, Some(62.0));

        let text2 = r#"```json
{"player": "Test2", "pick": "Under", "line": 100.0, "prop_category": "Yards", "win_probability": 62}
```"#;
        let preds2 = tracker.extract_predictions("s", text2);
        assert_eq!(preds2[0].probability, Some(62.0));
    }

    #[test]
    fn test_multiple_json_blocks_in_response() {
        let tracker = PredictionTracker__extraction_only();
        let text = r#"Here are my picks:

```json
{"player": "Mahomes", "pick": "Over", "line": 285.5, "prop_category": "Passing Yards", "confidence_score": 72, "win_probability": 62, "reasoning": "Elite matchup", "key_risk": "Wind"}
```

And another:

```json
{"player": "Barkley", "pick": "Over", "line": 88.5, "prop_category": "Rushing Yards", "confidence_score": 68, "win_probability": 60, "reasoning": "Workhorse", "key_risk": "Game script"}
```"#;

        let preds = tracker.extract_predictions("test", text);
        assert_eq!(preds.len(), 2);
        assert_eq!(preds[0].player_name, Some("Mahomes".to_string()));
        assert_eq!(preds[1].player_name, Some("Barkley".to_string()));
    }

    /// Helper: create a minimal tracker for extraction-only tests.
    /// Uses an in-memory SQLite database.
    fn PredictionTracker__extraction_only() -> PredictionTracker {
        // Create a tokio runtime for the test
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            // Create the schema
            sqlx::query(
                r#"
                CREATE TABLE IF NOT EXISTS predictions (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    raw_text TEXT NOT NULL DEFAULT '',
                    player_name TEXT,
                    pick_type TEXT,
                    line REAL,
                    stat_category TEXT,
                    confidence TEXT,
                    confidence_score INTEGER,
                    probability REAL,
                    reasoning TEXT,
                    risk TEXT,
                    created_at TEXT NOT NULL,
                    outcome TEXT NOT NULL DEFAULT 'Pending',
                    actual_result REAL,
                    notes TEXT,
                    resolved_at TEXT
                )
                "#,
            )
            .execute(&pool)
            .await
            .unwrap();
            PredictionTracker { pool }
        })
    }
}
