//! Analysis Context Builder — Injects mathematical analysis into AI prompts
//!
//! Bridges the analysis engine (edge calculator, matchup analyzer,
//! parlay correlation, prop scorer) with the chat system so the AI
//! receives computed mathematical context for sharper predictions.

use super::edge_calculator::{self, EdgeScore};
use super::matchup_analyzer::{self, MatchupAnalysis};
use super::parlay_correlation::{self, ParlayAnalysis};
use super::prop_scorer::{self, ScoredProp};
use crate::correlation::CorrelationPick;

/// Complete analysis context for injection into the AI system prompt
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalysisContext {
    pub edge_scores: Vec<EdgeScore>,
    pub matchup_analyses: Vec<MatchupAnalysis>,
    pub parlay_analysis: Option<ParlayAnalysis>,
    pub scored_props: Vec<ScoredProp>,
    pub ml_predictions: Vec<crate::ml_predictor::MLPrediction>,
    pub ml_model_accuracy: Option<f64>,
}

impl AnalysisContext {
    /// Generate a compact context string for AI prompt injection
    pub fn to_prompt_context(&self) -> String {
        let mut ctx = String::with_capacity(4096);

        // Prop scoring summary (most important for AI)
        if !self.scored_props.is_empty() {
            ctx.push_str(&prop_scorer::generate_scoring_context(&self.scored_props));
            ctx.push('\n');
        }

        // Edge scores (if no scored props available)
        if self.scored_props.is_empty() && !self.edge_scores.is_empty() {
            ctx.push_str("## MATHEMATICAL EDGE ANALYSIS\n");
            for edge in self.edge_scores.iter().take(8) {
                let emoji = if edge.quality_tier == "Elite" {
                    "🔥"
                } else if edge.quality_tier == "Strong" {
                    "💪"
                } else if edge.quality_tier == "Playable" {
                    "👍"
                } else {
                    "🤔"
                };
                ctx.push_str(&format!(
                    "{} {} {} {} — Edge: {:.1}%, Win Prob: {:.0}%, EV: {:.1}%, Kelly: {:.2}%, Conf: {} ({})\n",
                    emoji,
                    edge.player_name,
                    edge.pick_type,
                    edge.stat_category,
                    edge.edge_pct,
                    edge.win_probability,
                    edge.expected_value,
                    edge.kelly_pct,
                    edge.confidence,
                    edge.quality_tier
                ));
                for factor in &self.edge_scores[0].factors {
                    if factor.impact.abs() > 1.5 {
                        let sign = if factor.impact > 0.0 { "+" } else { "" };
                        ctx.push_str(&format!(
                            "   {}{}%: {}\n",
                            sign, factor.impact, factor.detail
                        ));
                    }
                }
            }
            ctx.push('\n');
        }

        // Matchup analyses
        if !self.matchup_analyses.is_empty() {
            ctx.push_str("## MATCHUP ANALYSIS\n");
            for ma in &self.matchup_analyses {
                ctx.push_str(&matchup_analyzer::generate_matchup_context(ma));
            }
            ctx.push('\n');
        }

        // Parlay correlation analysis
        if let Some(ref pa) = self.parlay_analysis {
            ctx.push_str(&parlay_correlation::generate_parlay_context(pa));
            ctx.push('\n');
        }

        // ML model predictions
        if !self.ml_predictions.is_empty() {
            let acc_str = self.ml_model_accuracy.map_or("N/A".to_string(), |a| format!("{:.1}%", a * 100.0));
            ctx.push_str(&format!("🤖 ML MODEL PREDICTIONS (accuracy: {}):\n", acc_str));
            for pred in self.ml_predictions.iter().take(10) {
                let emoji = if pred.ml_win_probability >= 0.6 {
                    "✅"
                } else if pred.ml_win_probability >= 0.45 {
                    "⚠️"
                } else {
                    "❌"
                };
                ctx.push_str(&format!(
                    "  {} {} {} {} — ML Win Prob: {:.1}% ({}), Line: {:.1}\n",
                    emoji,
                    pred.player_name,
                    pred.ml_prediction,
                    pred.stat_category,
                    pred.ml_win_probability * 100.0,
                    if pred.ml_win_probability >= 0.5 { "Lean Over" } else { "Lean Under" },
                    pred.line
                ));
            }
            ctx.push('\n');
        }

        ctx
    }
}

/// Input for running a full analysis pipeline
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalysisInput {
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
    /// Optional: consistency score (0-100) from historical data
    pub consistency_score: Option<f64>,
}

/// Run the full analysis pipeline on a single prop
pub fn analyze_single_prop(input: &AnalysisInput) -> (EdgeScore, ScoredProp) {
    let mut edge = edge_calculator::calculate_edge(
        &input.player_name,
        &input.stat_category,
        input.line,
        input.projection,
        input.season_avg,
        input.last3_avg,
        input.home_avg,
        input.away_avg,
        input.is_home,
        input.defense_rank,
        input.pace_rank,
        input.usage_rate,
        input.opponent_pace_rank,
        input.park_factor,
        input.goalie_quality_rank,
        &input.pick_type,
    );

    // Apply the measured calibration (backtested against 2022-2024 NFL
    // outcomes; see prizepicks-monster/reports/backtest-report.md). Raw
    // probability stays in edge.raw_win_probability and the adjustment is
    // listed as a factor.
    if let Some(cal) = super::calibration::current() {
        edge_calculator::apply_calibration(&mut edge, cal);
    }

    let scored = prop_scorer::score_prop(&edge, None, input.consistency_score);

    (edge, scored)
}

/// Analyze multiple props and return scored + ranked results
pub fn analyze_multiple_props(inputs: &[AnalysisInput]) -> AnalysisContext {
    let mut edges = Vec::with_capacity(inputs.len());
    let mut scored = Vec::with_capacity(inputs.len());

    for input in inputs {
        let (edge, score) = analyze_single_prop(input);
        edges.push(edge);
        scored.push(score);
    }

    let scored = prop_scorer::score_and_rank_props(scored);

    AnalysisContext {
        edge_scores: edges,
        matchup_analyses: vec![],
        parlay_analysis: None,
        scored_props: scored,
        ml_predictions: vec![],
        ml_model_accuracy: None,
    }
}

/// Analyze parlay correlation for a set of picks
pub fn analyze_parlay(picks: &[CorrelationPick]) -> ParlayAnalysis {
    parlay_correlation::analyze_parlay(picks, 1.909, 0.25)
}

/// Generate analysis context from picks (for parlay building)
pub fn analyze_parlay_context(picks: &[CorrelationPick]) -> String {
    let analysis = analyze_parlay(picks);
    parlay_correlation::generate_parlay_context(&analysis)
}
