#![allow(dead_code)]
//! Prop Scorer — Automated Prop Quality Scoring Engine
//!
//! Scores and ranks player props by combining multiple signals:
//!   - Edge calculator output (mathematical edge)
//!   - Matchup analysis (situational factors)
//!   - Historical performance at similar lines
//!   - Market efficiency indicators
//!
//! Output: A composite score (0-100) with tier classification
//! that the AI can use to prioritize which props to recommend.

use crate::analysis::edge_calculator::EdgeScore;
use crate::analysis::matchup_analyzer::MatchupAnalysis;
use serde::{Deserialize, Serialize};

/// A fully scored prop with all analysis dimensions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredProp {
    pub player_name: String,
    pub stat_category: String,
    pub line: f64,
    pub pick_type: String,
    pub composite_score: f64,
    pub edge_score: f64,
    pub matchup_score: f64,
    pub consistency_score: f64,
    pub value_score: f64,
    pub tier: PropTier,
    pub win_probability: f64,
    pub expected_value: f64,
    pub kelly_stake_pct: f64,
    pub confidence: String,
    pub key_factors: Vec<String>,
    pub risks: Vec<String>,
    pub recommendation: String,
}

/// Prop quality tier
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PropTier {
    Elite,
    Strong,
    Playable,
    Marginal,
    Avoid,
}

impl PropTier {
    pub fn as_str(&self) -> &str {
        match self {
            PropTier::Elite => "Elite",
            PropTier::Strong => "Strong",
            PropTier::Playable => "Playable",
            PropTier::Marginal => "Marginal",
            PropTier::Avoid => "Avoid",
        }
    }

    pub fn emoji(&self) -> &str {
        match self {
            PropTier::Elite => "🔥",
            PropTier::Strong => "💪",
            PropTier::Playable => "👍",
            PropTier::Marginal => "🤔",
            PropTier::Avoid => "❌",
        }
    }
}

impl std::fmt::Display for PropTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Score a prop by combining edge calculator and matchup analysis
pub fn score_prop(
    edge: &EdgeScore,
    matchup: Option<&MatchupAnalysis>,
    consistency_score: Option<f64>,
) -> ScoredProp {
    let edge_normalized = if edge.expected_value > 10.0 {
        95.0
    } else if edge.expected_value > 5.0 {
        80.0 + (edge.expected_value - 5.0) * 3.0
    } else if edge.expected_value > 2.0 {
        65.0 + (edge.expected_value - 2.0) * 5.0
    } else if edge.expected_value > 0.0 {
        50.0 + edge.expected_value * 7.5
    } else if edge.expected_value > -2.0 {
        50.0 + edge.expected_value * 10.0
    } else {
        (30.0 + edge.expected_value * 5.0).max(5.0)
    }
    .clamp(0.0, 100.0);

    let (matchup_normalized, matchup_factors) = if let Some(m) = matchup {
        let difficulty_score = (1.0 - m.overall_difficulty) * 100.0;
        let adj_bonus = (m.defensive_adjustment as f64).clamp(-10.0, 10.0) * 2.0;
        let score = (difficulty_score + adj_bonus as f64).clamp(0.0_f64, 100.0_f64);
        let factors: Vec<String> = m
            .situational_factors
            .iter()
            .map(|f| format!("{}: {}", f.label, f.detail))
            .collect();
        (score, factors)
    } else {
        (50.0, vec![])
    };

    let consistency_normalized = consistency_score.unwrap_or(50.0).clamp(0.0, 100.0);

    let value_normalized = if edge.kelly_pct > 4.0 {
        95.0
    } else if edge.kelly_pct > 2.5 {
        80.0 + (edge.kelly_pct - 2.5) * 10.0
    } else if edge.kelly_pct > 1.0 {
        60.0 + (edge.kelly_pct - 1.0) * 13.3
    } else if edge.kelly_pct > 0.0 {
        40.0 + edge.kelly_pct * 20.0
    } else {
        20.0
    }
    .clamp(0.0, 100.0);

    let composite = edge_normalized * 0.40
        + matchup_normalized * 0.30
        + consistency_normalized * 0.15
        + value_normalized * 0.15;

    let tier = if composite >= 80.0 {
        PropTier::Elite
    } else if composite >= 65.0 {
        PropTier::Strong
    } else if composite >= 50.0 {
        PropTier::Playable
    } else if composite >= 35.0 {
        PropTier::Marginal
    } else {
        PropTier::Avoid
    };

    let mut key_factors = Vec::new();
    key_factors.push(format!(
        "Edge: {:.1}% EV, {} confidence",
        edge.expected_value, edge.confidence
    ));
    key_factors.push(format!(
        "Win Prob: {:.0}%, Kelly Stake: {:.2}%",
        edge.win_probability, edge.kelly_pct
    ));
    key_factors.extend(matchup_factors);
    for factor in &edge.factors {
        if factor.impact.abs() > 2.0 {
            key_factors.push(format!("{}: {}", factor.label, factor.detail));
        }
    }

    let recommendation = match &tier {
        PropTier::Elite => format!(
            "🔥 ELITE PICK: {} {} {} — Score: {:.0}/100. EV: {:.1}%. Stake: {:.2}% of bankroll.",
            edge.player_name, edge.stat_category, edge.pick_type, composite, edge.expected_value, edge.kelly_pct
        ),
        PropTier::Strong => format!(
            "💪 STRONG PICK: {} {} {} — Score: {:.0}/100. EV: {:.1}%. Stake: {:.2}% of bankroll.",
            edge.player_name, edge.stat_category, edge.pick_type, composite, edge.expected_value, edge.kelly_pct
        ),
        PropTier::Playable => format!(
            "👍 PLAYABLE: {} {} {} — Score: {:.0}/100. EV: {:.1}%. Stake: {:.2}% of bankroll.",
            edge.player_name, edge.stat_category, edge.pick_type, composite, edge.expected_value, edge.kelly_pct
        ),
        PropTier::Marginal => format!(
            "🤔 MARGINAL: {} {} {} — Score: {:.0}/100. Weak edge.",
            edge.player_name, edge.stat_category, edge.pick_type, composite
        ),
        PropTier::Avoid => format!(
            "❌ AVOID: {} {} {} — Score: {:.0}/100. No edge.",
            edge.player_name, edge.stat_category, edge.pick_type, composite
        ),
    };

    ScoredProp {
        player_name: edge.player_name.clone(),
        stat_category: edge.stat_category.clone(),
        line: edge.line,
        pick_type: edge.pick_type.clone(),
        composite_score: (composite * 10.0).round() / 10.0,
        edge_score: (edge_normalized * 10.0).round() / 10.0,
        matchup_score: (matchup_normalized * 10.0).round() / 10.0,
        consistency_score: (consistency_normalized * 10.0).round() / 10.0,
        value_score: (value_normalized * 10.0).round() / 10.0,
        tier,
        win_probability: edge.win_probability,
        expected_value: edge.expected_value,
        kelly_stake_pct: edge.kelly_pct,
        confidence: edge.confidence.clone(),
        key_factors,
        risks: edge.risks.clone(),
        recommendation,
    }
}

/// Score and rank multiple props, returning them sorted by composite score
pub fn score_and_rank_props(mut props: Vec<ScoredProp>) -> Vec<ScoredProp> {
    props.sort_by(|a, b| {
        b.composite_score
            .partial_cmp(&a.composite_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    props
}

/// Filter props by minimum tier
pub fn filter_by_tier<'a>(props: &'a [ScoredProp], min_tier: &PropTier) -> Vec<&'a ScoredProp> {
    let min_score = match min_tier {
        PropTier::Elite => 80.0,
        PropTier::Strong => 65.0,
        PropTier::Playable => 50.0,
        PropTier::Marginal => 35.0,
        PropTier::Avoid => 0.0,
    };
    props.iter().filter(|p| p.composite_score >= min_score).collect()
}

/// Generate a compact scoring summary for AI context injection
pub fn generate_scoring_context(scored_props: &[ScoredProp]) -> String {
    if scored_props.is_empty() {
        return "No scored props available.".to_string();
    }

    let mut ctx = String::from("📊 PROP SCORING RESULTS:\n");
    for (i, prop) in scored_props.iter().take(10).enumerate() {
        ctx.push_str(&format!(
            "  {}. {} {} {} — Score: {:.0}/100 ({}), EV: {:.1}%, Win Prob: {:.0}%, Stake: {:.2}%\n",
            i + 1,
            prop.tier.emoji(),
            prop.player_name,
            prop.stat_category,
            prop.composite_score,
            prop.tier,
            prop.expected_value,
            prop.win_probability,
            prop.kelly_stake_pct,
        ));
    }

    let elite_count = scored_props.iter().filter(|p| p.tier == PropTier::Elite).count();
    let strong_count = scored_props.iter().filter(|p| p.tier == PropTier::Strong).count();
    let playable_count = scored_props.iter().filter(|p| p.tier == PropTier::Playable).count();

    ctx.push_str(&format!(
        "\n  Summary: {} Elite, {} Strong, {} Playable out of {} total props\n",
        elite_count,
        strong_count,
        playable_count,
        scored_props.len()
    ));

    ctx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::edge_calculator::EdgeScore;

    fn make_edge_score(name: &str, ev: f64, win_prob: f64, kelly: f64) -> EdgeScore {
        EdgeScore {
            player_name: name.into(),
            stat_category: "Passing Yards".into(),
            line: 250.0,
            projection: 260.0,
            edge_pct: 4.0,
            win_probability: win_prob,
            expected_value: ev,
            kelly_pct: kelly,
            confidence: "Medium".into(),
            confidence_score: 65,
            quality_tier: "Strong".into(),
            pick_type: "Over".into(),
            factors: vec![],
            risks: vec![],
            raw_win_probability: win_prob,
            calibration: None,
        }
    }

    #[test]
    fn test_elite_prop() {
        let edge = make_edge_score("Elite Player", 12.0, 68.0, 4.5);
        let scored = score_prop(&edge, None, Some(85.0));
        assert_eq!(scored.tier, PropTier::Elite);
        assert!(scored.composite_score >= 80.0);
    }

    #[test]
    fn test_avoid_prop() {
        let edge = make_edge_score("Bad Prop", -5.0, 42.0, 0.0);
        let scored = score_prop(&edge, None, Some(30.0));
        assert_eq!(scored.tier, PropTier::Avoid);
    }

    #[test]
    fn test_score_and_rank() {
        let props = vec![
            score_prop(&make_edge_score("A", 3.0, 55.0, 1.5), None, None),
            score_prop(&make_edge_score("B", 10.0, 65.0, 4.0), None, None),
            score_prop(&make_edge_score("C", 1.0, 52.0, 0.5), None, None),
        ];
        let ranked = score_and_rank_props(props);
        assert_eq!(ranked[0].player_name, "B");
        assert_eq!(ranked[2].player_name, "C");
    }

    #[test]
    fn test_filter_by_tier() {
        let props = vec![
            score_prop(&make_edge_score("A", 12.0, 68.0, 4.5), None, None),
            score_prop(&make_edge_score("B", 5.0, 58.0, 2.0), None, None),
            score_prop(&make_edge_score("C", -2.0, 45.0, 0.0), None, None),
        ];
        let strong_or_better = filter_by_tier(&props, &PropTier::Strong);
        assert!(strong_or_better.len() >= 1);
    }
}
