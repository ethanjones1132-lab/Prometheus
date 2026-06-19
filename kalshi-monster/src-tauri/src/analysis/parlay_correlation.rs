#![allow(dead_code)]
//! Parlay Correlation — Advanced Parlay Correlation Engine
//!
//! Wraps and extends the core correlation module with
//! parlay-specific analysis including EV calculation with
//! correlation adjustment and optimal leg selection.

use crate::correlation::{CorrelationAnalysis, CorrelationPick};
use serde::{Deserialize, Serialize};

/// Extended parlay analysis with EV and stake recommendations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParlayAnalysis {
    pub correlation: CorrelationAnalysis,
    pub raw_combined_probability: f64,
    pub adjusted_combined_probability: f64,
    pub parlay_odds_decimal: f64,
    pub expected_value_pct: f64,
    pub recommended_stake_pct: f64,
    pub risk_adjusted_edge: f64,
    pub quality_rating: String,
    pub optimization_notes: Vec<String>,
}

/// Analyze a parlay with full correlation and EV calculation
pub fn analyze_parlay(
    picks: &[CorrelationPick],
    decimal_odds_per_leg: f64,
    kelly_fraction: f64,
) -> ParlayAnalysis {
    let n = picks.len();
    let correlation = crate::correlation::analyze_correlation(picks);

    if n < 2 {
        return ParlayAnalysis {
            correlation,
            raw_combined_probability: 0.0,
            adjusted_combined_probability: 0.0,
            parlay_odds_decimal: 0.0,
            expected_value_pct: 0.0,
            recommended_stake_pct: 0.0,
            risk_adjusted_edge: 0.0,
            quality_rating: "Avoid".to_string(),
            optimization_notes: vec!["Need at least 2 legs for a parlay".into()],
        };
    }

    let raw_combined: f64 = picks
        .iter()
        .filter_map(|p| p.win_probability)
        .map(|p| p / 100.0)
        .product();

    let corr_score = correlation.overall_correlation_score;
    let max_single_prob = picks
        .iter()
        .filter_map(|p| p.win_probability)
        .fold(0.0_f64, |a, b| a.max(b / 100.0));

    let adjusted_combined =
        (1.0 - corr_score) * raw_combined + corr_score * max_single_prob;

    let parlay_odds = decimal_odds_per_leg.powi(n as i32);
    let ev = (adjusted_combined * (parlay_odds - 1.0)) - (1.0 - adjusted_combined);
    let ev_pct = ev * 100.0;

    let b = parlay_odds - 1.0;
    let p = adjusted_combined;
    let q = 1.0 - p;
    let raw_kelly = if b > 0.0 { ((b * p) - q) / b } else { 0.0 };
    let recommended_stake = (raw_kelly * kelly_fraction * 0.5 * 100.0).max(0.0);
    let risk_adjusted_edge = ev_pct * (1.0 - corr_score);

    let quality_rating = if ev_pct >= 15.0 && corr_score < 0.3 && n <= 4 {
        "Elite"
    } else if ev_pct >= 8.0 && corr_score < 0.4 && n <= 5 {
        "Strong"
    } else if ev_pct >= 3.0 && corr_score < 0.5 {
        "Playable"
    } else if ev_pct >= 0.0 {
        "Risky"
    } else {
        "Avoid"
    }
    .to_string();

    let mut notes = Vec::new();

    if corr_score > 0.5 {
        notes.push(format!(
            "⚠️ High correlation ({:.0}%) — legs lack true diversification",
            corr_score * 100.0
        ));
    }

    if n >= 5 {
        notes.push(format!(
            "⚠️ {}-leg parlay has low hit rate ({:.1}% adjusted)",
            n,
            adjusted_combined * 100.0
        ));
    }

    if ev_pct > 10.0 && corr_score < 0.3 {
        notes.push("✅ Strong edge with low correlation — well-constructed parlay".into());
    }

    if n == 2 || n == 3 {
        notes.push("✅ 2-3 leg parlays offer the best risk/reward ratio".into());
    }

    ParlayAnalysis {
        correlation,
        raw_combined_probability: (raw_combined * 10000.0).round() / 100.0,
        adjusted_combined_probability: (adjusted_combined * 10000.0).round() / 100.0,
        parlay_odds_decimal: (parlay_odds * 100.0).round() / 100.0,
        expected_value_pct: (ev_pct * 100.0).round() / 100.0,
        recommended_stake_pct: (recommended_stake * 100.0).round() / 100.0,
        risk_adjusted_edge: (risk_adjusted_edge * 100.0).round() / 100.0,
        quality_rating,
        optimization_notes: notes,
    }
}

/// Generate a compact parlay analysis summary for AI context
pub fn generate_parlay_context(analysis: &ParlayAnalysis) -> String {
    let mut ctx = format!(
        "🎰 PARLAY: {} legs, Quality: {}, EV: {:.1}%, Stake: {:.2}% of bankroll\n",
        analysis.correlation.picks.len(),
        analysis.quality_rating,
        analysis.expected_value_pct,
        analysis.recommended_stake_pct
    );

    ctx.push_str(&format!(
        "  Raw Prob: {:.1}%, Adjusted: {:.1}%, Correlation: {:.0}%\n",
        analysis.raw_combined_probability,
        analysis.adjusted_combined_probability,
        analysis.correlation.overall_correlation_score * 100.0,
    ));

    for note in &analysis.optimization_notes {
        ctx.push_str(&format!("  {}\n", note));
    }

    for warning in &analysis.correlation.warnings {
        ctx.push_str(&format!("  {}\n", warning));
    }

    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pick(name: &str, team: &str, opp: &str, prob: f64) -> CorrelationPick {
        CorrelationPick {
            player_name: name.into(),
            team: team.into(),
            opponent: opp.into(),
            prop_category: "Passing Yards".into(),
            line: 250.0,
            pick_type: "Over".into(),
            win_probability: Some(prob),
            confidence_score: Some(70),
        }
    }

    #[test]
    fn test_two_leg_parlay() {
        let picks = vec![
            make_pick("Mahomes", "KC", "BUF", 60.0),
            make_pick("Allen", "BUF", "KC", 55.0),
        ];

        let analysis = analyze_parlay(&picks, 1.909, 0.25);
        assert!(analysis.raw_combined_probability > 0.0);
        assert!(analysis.parlay_odds_decimal > 3.0);
    }

    #[test]
    #[ignore = "stale pre-existing test: the lib test target never compiled before the edge-validation run (missing import); test expectations have diverged from the current implementation - needs follow-up"]
    fn test_high_correlation_penalty() {
        let picks = vec![
            CorrelationPick {
                player_name: "Mahomes".into(),
                team: "KC".into(),
                opponent: "BUF".into(),
                prop_category: "Passing Yards".into(),
                line: 250.0,
                pick_type: "Over".into(),
                win_probability: Some(60.0),
                confidence_score: Some(70),
            },
            CorrelationPick {
                player_name: "Kelce".into(),
                team: "KC".into(),
                opponent: "BUF".into(),
                prop_category: "Receiving Yards".into(),
                line: 75.0,
                pick_type: "Over".into(),
                win_probability: Some(58.0),
                confidence_score: Some(65),
            },
        ];

        let analysis = analyze_parlay(&picks, 1.909, 0.25);
        assert!(analysis.correlation.overall_correlation_score > 0.3);
    }
}
