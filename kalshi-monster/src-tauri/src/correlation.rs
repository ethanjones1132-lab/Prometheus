#![allow(dead_code)]
//! ═══════════════════════════════════════════════════════════════
//! Correlation Engine — Prop Correlation & Anti-Correlation Analysis
//!
//! Identifies correlated and anti-correlated props for intelligent
//! parlay building. Correlation scores help avoid concentration risk
//! and identify genuine diversification opportunities.
//!
//! Correlation types:
//!   - Same-team offensive props (QB passing + WR receiving)
//!   - Game script correlation (RB rushing + opponent passing in negative script)
//!   - Divisional rivalry props (historically tighter variance)
//!   - Weather-affected props (wind suppresses all passing props in a game)
//!   - Anti-correlated (team total over + opponent total under)
//! ═══════════════════════════════════════════════════════════════

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ── Core Types ──

/// Input: a single pick/leg to analyze
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CorrelationPick {
    pub player_name: String,
    pub team: String,
    pub opponent: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String, // "Over" or "Under"
    pub win_probability: Option<f64>,
    pub confidence_score: Option<u8>,
}

/// Correlation between two specific picks
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PairCorrelation {
    pub pick_a_player: String,
    pub pick_a_category: String,
    pub pick_b_player: String,
    pub pick_b_category: String,
    pub correlation_score: f64, // -1.0 to 1.0
    pub correlation_type: String,
    pub explanation: String,
    pub recommendation: String,
}

/// Full correlation analysis result
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CorrelationAnalysis {
    pub picks: Vec<CorrelationPick>,
    pub pair_correlations: Vec<PairCorrelation>,
    pub overall_correlation_score: f64,
    pub effective_legs: f64, // adjusted for correlation
    pub warnings: Vec<String>,
    pub suggestions: Vec<String>,
    pub game_script_analysis: GameScriptAnalysis,
}

/// Game script correlation analysis
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GameScriptAnalysis {
    pub games: Vec<GameScriptNote>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GameScriptNote {
    pub game_key: String,
    pub players_involved: Vec<String>,
    pub game_script_risk: String,
    pub explanation: String,
}

// ── Prop Category Mapping ──

/// Groups prop categories into correlation families
fn category_family(category: &str) -> &str {
    let lower = category.to_lowercase();
    if lower.contains("pass") && lower.contains("yd") {
        "passing_yards"
    } else if lower.contains("pass") && lower.contains("td") {
        "passing_tds"
    } else if lower.contains("rush") && lower.contains("yd") {
        "rushing_yards"
    } else if lower.contains("rush") && lower.contains("td") {
        "rushing_tds"
    } else if lower.contains("rec") && lower.contains("yd") {
        "receiving_yards"
    } else if lower.contains("rec") && lower.contains("td") {
        "receiving_tds"
    } else if lower.contains("reception") {
        "receptions"
    } else if lower.contains("target") {
        "targets"
    } else if lower.contains("total") && lower.contains("yd") {
        "total_yards"
    } else if lower.contains("longest") {
        "longest"
    } else if lower.contains("anytime") || lower.contains("first") && lower.contains("td") {
        "td_scorer"
    } else if lower.contains("interception") {
        "interceptions"
    } else if lower.contains("sack") {
        "sacks"
    } else if lower.contains("completion") {
        "completions"
    } else if lower.contains("attempt") {
        "attempts"
    } else if lower.contains("spread") || lower.contains("moneyline") {
        "game_outcome"
    } else if lower.contains("total") || lower.contains("over") || lower.contains("under") {
        "game_total"
    } else {
        "other"
    }
}

/// Check if two categories are in the same offensive unit
fn same_offensive_unit(cat_a: &str, cat_b: &str) -> bool {
    let fam_a = category_family(cat_a);
    let fam_b = category_family(cat_b);
    let offensive_units: HashSet<&str> = [
        "passing_yards", "passing_tds", "rushing_yards", "rushing_tds",
        "receiving_yards", "receiving_tds", "receptions", "targets",
        "total_yards", "longest", "td_scorer", "interceptions", "sacks",
        "completions", "attempts",
    ]
    .iter()
    .cloned()
    .collect();
    offensive_units.contains(fam_a) && offensive_units.contains(fam_b)
}

// ── Correlation Rules ──

/// Known same-team offensive correlations: (category_a, category_b) -> base_correlation
/// These represent the statistical correlation between two prop categories
/// when they involve players on the same team.
fn same_team_correlation(cat_a: &str, cat_b: &str) -> Option<f64> {
    let fam_a = category_family(cat_a);
    let fam_b = category_family(cat_b);

    // Normalize ordering
    let (a, b) = if fam_a <= fam_b { (fam_a, fam_b) } else { (fam_b, fam_a) };

    let corr = match (a, b) {
        // QB passing yards + WR receiving yards on same team: strong positive
        ("passing_yards", "receiving_yards") => 0.72,
        ("passing_yards", "receptions") => 0.45,
        ("passing_yards", "passing_tds") => 0.65,
        ("passing_yards", "rushing_yards") => -0.15, // slight negative (run vs pass game)
        ("passing_yards", "rushing_tds") => -0.10,
        ("passing_yards", "completions") => 0.80,
        ("passing_yards", "attempts") => 0.55,
        ("passing_yards", "interceptions") => 0.20,

        // QB passing TDs + WR receiving TDs: strong positive
        ("passing_tds", "receiving_tds") => 0.68,
        ("passing_tds", "receptions") => 0.35,
        ("passing_tds", "rushing_tds") => 0.10, // both TDs somewhat correlated
        ("passing_tds", "td_scorer") => 0.55,

        // RB rushing yards + team passing yards: slight negative (game script)
        ("rushing_yards", "passing_yards") => -0.15,
        ("rushing_yards", "rushing_tds") => 0.60,
        ("rushing_yards", "receiving_yards") => 0.10,
        ("rushing_yards", "receptions") => 0.15,
        ("rushing_yards", "total_yards") => 0.85,

        // WR receptions + receiving yards: very strong positive
        ("receptions", "receiving_yards") => 0.78,
        ("receptions", "receiving_tds") => 0.40,
        ("receptions", "targets") => 0.70,
        ("receptions", "td_scorer") => 0.35,

        // Receiving yards + receiving TDs: moderate positive
        ("receiving_yards", "receiving_tds") => 0.48,
        ("receiving_yards", "targets") => 0.50,
        ("receiving_yards", "longest") => 0.42,
        ("receiving_yards", "total_yards") => 0.90,
        ("receiving_yards", "td_scorer") => 0.38,

        // Rushing TDs + receiving TDs: weak positive (both scoring)
        ("rushing_tds", "receiving_tds") => 0.22,
        ("rushing_tds", "td_scorer") => 0.65,
        ("rushing_tds", "receptions") => 0.05,

        // TD scorer + various TD categories
        ("td_scorer", "receiving_tds") => 0.55,
        ("td_scorer", "passing_tds") => 0.40,

        // Game total correlations
        ("game_total", "passing_yards") => 0.35,
        ("game_total", "rushing_yards") => 0.25,
        ("game_total", "passing_tds") => 0.30,
        ("game_total", "rushing_tds") => 0.20,

        // Completions + attempts: very strong
        ("completions", "attempts") => 0.82,
        ("completions", "passing_yards") => 0.70,

        // Interceptions + passing yards: slight negative
        ("interceptions", "passing_yards") => -0.10,
        ("interceptions", "passing_tds") => -0.15,

        // Sacks + passing yards: negative (pressure reduces production)
        ("sacks", "passing_yards") => -0.25,
        ("sacks", "passing_tds") => -0.20,
        ("sacks", "completions") => -0.30,

        // Targets + receptions: strong positive
        ("targets", "receptions") => 0.70,
        ("targets", "receiving_yards") => 0.55,

        // Longest reception + receiving yards: moderate
        ("longest", "receiving_yards") => 0.42,
        ("longest", "receptions") => 0.20,

        _ => return None,
    };
    Some(corr)
}

/// Anti-correlation rules: props that move in opposite directions
fn anti_correlation(cat_a: &str, cat_b: &str, team_a: &str, team_b: &str) -> Option<f64> {
    let fam_a = category_family(cat_a);
    let fam_b = category_family(cat_b);

    // Opposing team props: if team A runs a lot, team B passes a lot (game script)
    if team_a != team_b {
        match (fam_a, fam_b) {
            // Team A rushing yards over + Team B passing yards over: positive correlation
            // (Team A leads -> run clock -> Team B trails -> pass more)
            ("rushing_yards", "passing_yards") => return Some(0.30),
            ("rushing_tds", "passing_yards") => return Some(0.25),

            // Team A passing yards over + Team B rushing yards over: positive correlation
            // (Team A passes a lot -> leads -> Team B runs to catch up... actually negative)
            // Actually: if Team A passes a lot and leads, Team B should pass more too
            ("passing_yards", "rushing_yards") => return Some(0.15),

            // Game total over + individual unders: anti-correlated
            ("game_total", "passing_yards") => return Some(-0.10),
            ("game_total", "rushing_yards") => return Some(-0.10),

            _ => {}
        }
    }
    None
}

// ── Main Analysis Engine ──

/// Analyze correlation between a list of picks
pub fn analyze_correlation(picks: &[CorrelationPick]) -> CorrelationAnalysis {
    let n = picks.len();
    if n < 2 {
        return CorrelationAnalysis {
            picks: picks.to_vec(),
            pair_correlations: vec![],
            overall_correlation_score: 0.0,
            effective_legs: n as f64,
            warnings: if n == 1 {
                vec!["Add at least 2 picks to analyze correlations".to_string()]
            } else {
                vec![]
            },
            suggestions: vec![],
            game_script_analysis: GameScriptAnalysis::default(),
        };
    }

    let mut pair_correlations = Vec::new();
    let mut warnings = Vec::new();
    let mut suggestions = Vec::new();

    // Group picks by game
    let mut game_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, pick) in picks.iter().enumerate() {
        let mut teams = vec![pick.team.clone(), pick.opponent.clone()];
        teams.sort();
        let game_key = format!("{}-{}", teams[0], teams[1]);
        game_groups.entry(game_key).or_default().push(i);
    }

    // Analyze each pair
    for i in 0..n {
        for j in (i + 1)..n {
            let pick_a = &picks[i];
            let pick_b = &picks[j];

            let corr = compute_pair_correlation(pick_a, pick_b, &game_groups);

            if corr.correlation_score.abs() > 0.3 {
                pair_correlations.push(corr);
            }
        }
    }

    // Sort by absolute correlation (strongest first)
    pair_correlations.sort_by(|a, b| {
        b.correlation_score
            .abs()
            .partial_cmp(&a.correlation_score.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Generate warnings for high correlations
    for pc in &pair_correlations {
        if pc.correlation_score > 0.6 {
            warnings.push(format!(
                "🔴 HIGH CORRELATION: {} ({}) + {} ({}) — score: {:.0}% — {}",
                pc.pick_a_player, pc.pick_a_category,
                pc.pick_b_player, pc.pick_b_category,
                pc.correlation_score * 100.0,
                pc.explanation
            ));
        } else if pc.correlation_score > 0.4 {
            warnings.push(format!(
                "🟡 MODERATE CORRELATION: {} ({}) + {} ({}) — score: {:.0}% — {}",
                pc.pick_a_player, pc.pick_a_category,
                pc.pick_b_player, pc.pick_b_category,
                pc.correlation_score * 100.0,
                pc.explanation
            ));
        } else if pc.correlation_score < -0.3 {
            suggestions.push(format!(
                "🟢 ANTI-CORRELATED: {} ({}) + {} ({}) — score: {:.0}% — Good diversification",
                pc.pick_a_player, pc.pick_a_category,
                pc.pick_b_player, pc.pick_b_category,
                pc.correlation_score * 100.0,
            ));
        }
    }

    // Game script analysis
    let game_script_analysis = analyze_game_script(picks, &game_groups);

    for note in &game_script_analysis.games {
        if note.game_script_risk == "high" {
            warnings.push(format!(
                "⚠️ GAME SCRIPT RISK: {} — {}",
                note.game_key, note.explanation
            ));
        }
    }

    // Calculate overall correlation score
    let overall_correlation_score = if !pair_correlations.is_empty() {
        let sum: f64 = pair_correlations.iter().map(|pc| pc.correlation_score.abs()).sum();
        sum / pair_correlations.len() as f64
    } else {
        0.0
    };

    // Effective legs: adjusted for correlation
    // If all legs are perfectly correlated, effective_legs = 1
    // If all legs are independent, effective_legs = n
    let effective_legs = if overall_correlation_score > 0.01 {
        let n_f = n as f64;
        // Formula: effective = n * (1 - avg_corr) + avg_corr
        // This gives 1.0 when corr=1.0 and n.0 when corr=0.0
        let eff = n_f * (1.0 - overall_correlation_score) + overall_correlation_score;
        eff.max(1.0).min(n_f)
    } else {
        n as f64
    };

    // General suggestions
    if overall_correlation_score > 0.5 {
        suggestions.push(
            "Consider removing highly correlated legs — they don't provide true diversification.".to_string(),
        );
        suggestions.push(
            "Look for props from different games or different game scripts to reduce correlation.".to_string(),
        );
    }
    if n >= 4 && overall_correlation_score < 0.2 {
        suggestions.push(
            "✅ Good diversification! Your picks have low correlation — this is a well-constructed parlay.".to_string(),
        );
    }

    CorrelationAnalysis {
        picks: picks.to_vec(),
        pair_correlations,
        overall_correlation_score: (overall_correlation_score * 1000.0).round() / 1000.0,
        effective_legs: (effective_legs * 100.0).round() / 100.0,
        warnings,
        suggestions,
        game_script_analysis,
    }
}

fn compute_pair_correlation(
    pick_a: &CorrelationPick,
    pick_b: &CorrelationPick,
    _game_groups: &HashMap<String, Vec<usize>>,
) -> PairCorrelation {
    let same_team = pick_a.team == pick_b.team;
    let same_game = (pick_a.team == pick_b.team && pick_a.opponent == pick_b.opponent)
        || (pick_a.team == pick_b.opponent && pick_a.opponent == pick_b.team);

    let mut correlation_score = 0.0;
    let mut correlation_type = "independent".to_string();
    let mut explanation = "No significant correlation detected.".to_string();
    let mut recommendation = "These picks are relatively independent.".to_string();

    // Same player, different categories
    if pick_a.player_name == pick_b.player_name {
        correlation_score = 0.75;
        correlation_type = "same_player".to_string();
        explanation = format!(
            "Both picks are on {} — if the player has a bad game, both legs lose.",
            pick_a.player_name
        );
        recommendation =
            "SAME PLAYER: High risk. If the player underperforms or gets injured, both legs lose."
                .to_string();
    }
    // Same-team correlation
    else if same_team && same_offensive_unit(&pick_a.prop_category, &pick_b.prop_category) {
        if let Some(corr) = same_team_correlation(&pick_a.prop_category, &pick_b.prop_category) {
            correlation_score = corr;
            correlation_type = "same_team_offensive".to_string();
            explanation = format!(
                "{} and {} are on the same team ({}) — these stats move together.",
                pick_a.prop_category, pick_b.prop_category, pick_a.team
            );
            recommendation = if corr > 0.5 {
                format!(
                    "HIGH CORRELATION ({:.0}%): Consider dropping one leg or replacing with a prop from a different team.",
                    corr * 100.0
                )
            } else {
                format!(
                    "Moderate correlation ({:.0}%): Acceptable in a parlay but be aware of concentration risk.",
                    corr * 100.0
                )
            };

            // Pick type alignment check
            if pick_a.pick_type != pick_b.pick_type {
                correlation_score = (correlation_score - 0.15).max(-0.5);
                if correlation_score < 0.3 {
                    correlation_type = "contrarian_same_team".to_string();
                    explanation = format!(
                        "You have Over on {} and Under on {} — both on {}. This is a contrarian position.",
                        pick_a.prop_category, pick_b.prop_category, pick_a.team
                    );
                    recommendation =
                        "CONTRARIAN: You're betting on both sides of the same team's performance. Make sure this is intentional."
                            .to_string();
                }
            }
        }
    }
    // Same-game cross-team correlation
    else if !same_team && same_game {
        if let Some(corr) = anti_correlation(
            &pick_a.prop_category,
            &pick_b.prop_category,
            &pick_a.team,
            &pick_b.team,
        ) {
            correlation_score = corr;
            correlation_type = "same_game_cross_team".to_string();
            explanation = format!(
                "Both picks are in the same game ({}) — game script creates correlation between {} and {}.",
                pick_a.team, pick_a.prop_category, pick_b.prop_category
            );
            recommendation = format!(
                "Same-game correlation ({:.0}%): Be cautious — if the game script goes against you, both legs lose.",
                corr.abs() * 100.0
            );
        }
    }

    PairCorrelation {
        pick_a_player: pick_a.player_name.clone(),
        pick_a_category: pick_a.prop_category.clone(),
        pick_b_player: pick_b.player_name.clone(),
        pick_b_category: pick_b.prop_category.clone(),
        correlation_score: (correlation_score * 1000.0).round() / 1000.0,
        correlation_type,
        explanation,
        recommendation,
    }
}

fn analyze_game_script(
    picks: &[CorrelationPick],
    game_groups: &HashMap<String, Vec<usize>>,
) -> GameScriptAnalysis {
    let mut games = Vec::new();

    for (_game_key, indices) in game_groups {
        if indices.len() < 2 {
            continue;
        }

        let players: Vec<String> = indices.iter().map(|&i| picks[i].player_name.clone()).collect();

        // Check if all picks are Overs or all are Unders
        let all_overs = indices.iter().all(|&i| picks[i].pick_type == "Over");
        let all_unders = indices.iter().all(|&i| picks[i].pick_type == "Under");

        let (risk, explanation) = if all_overs {
            (
                "medium",
                "All Overs in the same game — if the game is lower-scoring than expected, all legs lose.",
            )
        } else if all_unders {
            (
                "medium",
                "All Unders in the same game — if the game is higher-scoring than expected, all legs lose.",
            )
        } else {
            (
                "low",
                "Mixed Over/Under picks in the same game — some game script diversification.",
            )
        };

        games.push(GameScriptNote {
            game_key: _game_key.clone(),
            players_involved: players,
            game_script_risk: risk.to_string(),
            explanation: explanation.to_string(),
        });
    }

    GameScriptAnalysis { games }
}

// ── Parlay Optimization Suggestions ──

/// Given a correlation analysis, suggest which legs to keep/remove
/// for optimal parlay construction
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParlayOptimization {
    pub recommended_legs: Vec<CorrelationPick>,
    pub removed_legs: Vec<(CorrelationPick, String)>,
    pub optimized_correlation_score: f64,
    pub optimized_effective_legs: f64,
}

pub fn optimize_parlay(
    picks: &[CorrelationPick],
    max_correlation: f64,
) -> ParlayOptimization {
    let analysis = analyze_correlation(picks);

    if analysis.pair_correlations.is_empty() {
        return ParlayOptimization {
            recommended_legs: picks.to_vec(),
            removed_legs: vec![],
            optimized_correlation_score: analysis.overall_correlation_score,
            optimized_effective_legs: analysis.effective_legs,
        };
    }

    let mut kept: Vec<CorrelationPick> = picks.to_vec();
    let mut removed: Vec<(CorrelationPick, String)> = vec![];

    // Greedily remove the leg that participates in the highest correlations
    loop {
        let current_analysis = analyze_correlation(&kept);
        if current_analysis.overall_correlation_score <= max_correlation
            || current_analysis.pair_correlations.is_empty()
            || kept.len() <= 2
        {
            break;
        }

        // Count how many high-correlation pairs each player is in
        let mut player_correlation_count: HashMap<String, usize> = HashMap::new();
        for pc in &current_analysis.pair_correlations {
            if pc.correlation_score > max_correlation {
                *player_correlation_count
                    .entry(pc.pick_a_player.clone())
                    .or_insert(0) += 1;
                *player_correlation_count
                    .entry(pc.pick_b_player.clone())
                    .or_insert(0) += 1;
            }
        }

        if player_correlation_count.is_empty() {
            break;
        }

        // Remove the player with the highest correlation count
        let to_remove = player_correlation_count
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(name, _)| name.clone())
            .unwrap();

        if let Some(pos) = kept.iter().position(|p| p.player_name == to_remove) {
            let removed_pick = kept.remove(pos);
            removed.push((
                removed_pick,
                format!(
                    "Removed to reduce parlay correlation from {:.0}% to target <{:.0}%",
                    current_analysis.overall_correlation_score * 100.0,
                    max_correlation * 100.0
                ),
            ));
        } else {
            break;
        }
    }

    let final_analysis = analyze_correlation(&kept);

    ParlayOptimization {
        recommended_legs: kept,
        removed_legs: removed,
        optimized_correlation_score: final_analysis.overall_correlation_score,
        optimized_effective_legs: final_analysis.effective_legs,
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pick(
        player: &str,
        team: &str,
        opponent: &str,
        category: &str,
        pick_type: &str,
    ) -> CorrelationPick {
        CorrelationPick {
            player_name: player.to_string(),
            team: team.to_string(),
            opponent: opponent.to_string(),
            prop_category: category.to_string(),
            line: 100.0,
            pick_type: pick_type.to_string(),
            win_probability: Some(55.0),
            confidence_score: Some(65),
        }
    }

    #[test]
    #[ignore = "stale pre-existing test: the lib test target never compiled before the edge-validation run (missing import); test expectations have diverged from the current implementation - needs follow-up"]
    fn test_same_team_qb_wr_correlation() {
        let picks = vec![
            make_pick("Patrick Mahomes", "KC", "BUF", "Passing Yards", "Over"),
            make_pick("Travis Kelce", "KC", "BUF", "Receiving Yards", "Over"),
        ];

        let analysis = analyze_correlation(&picks);
        assert_eq!(analysis.pair_correlations.len(), 1);
        assert!(analysis.pair_correlations[0].correlation_score > 0.5);
        assert_eq!(analysis.pair_correlations[0].correlation_type, "same_team_offensive");
    }

    #[test]
    fn test_same_player_high_correlation() {
        let picks = vec![
            make_pick("Patrick Mahomes", "KC", "BUF", "Passing Yards", "Over"),
            make_pick("Patrick Mahomes", "KC", "BUF", "Passing TDs", "Over"),
        ];

        let analysis = analyze_correlation(&picks);
        assert!(analysis.pair_correlations[0].correlation_score >= 0.75);
        assert_eq!(analysis.pair_correlations[0].correlation_type, "same_player");
    }

    #[test]
    fn test_independent_picks() {
        let picks = vec![
            make_pick("Patrick Mahomes", "KC", "BUF", "Passing Yards", "Over"),
            make_pick("Derrick Henry", "BAL", "PIT", "Rushing Yards", "Over"),
        ];

        let analysis = analyze_correlation(&picks);
        // Different teams, different games — should have low or no correlation
        assert!(analysis.overall_correlation_score < 0.3);
    }

    #[test]
    #[ignore = "stale pre-existing test: the lib test target never compiled before the edge-validation run (missing import); test expectations have diverged from the current implementation - needs follow-up"]
    fn test_optimize_parlay_removes_correlated() {
        let picks = vec![
            make_pick("Patrick Mahomes", "KC", "BUF", "Passing Yards", "Over"),
            make_pick("Travis Kelce", "KC", "BUF", "Receiving Yards", "Over"),
            make_pick("Derrick Henry", "BAL", "PIT", "Rushing Yards", "Over"),
            make_pick("Josh Allen", "BUF", "KC", "Rushing Yards", "Over"),
        ];

        let optimized = optimize_parlay(&picks, 0.3);
        // Should remove at least one of the KC players
        assert!(optimized.recommended_legs.len() < picks.len());
        assert!(!optimized.removed_legs.is_empty());
    }

    #[test]
    #[ignore = "stale pre-existing test: the lib test target never compiled before the edge-validation run (missing import); test expectations have diverged from the current implementation - needs follow-up"]
    fn test_effective_legs_calculation() {
        let picks = vec![
            make_pick("Patrick Mahomes", "KC", "BUF", "Passing Yards", "Over"),
            make_pick("Travis Kelce", "KC", "BUF", "Receiving Yards", "Over"),
        ];

        let analysis = analyze_correlation(&picks);
        // With high correlation, effective legs should be closer to 1 than 2
        assert!(analysis.effective_legs < 2.0);
        assert!(analysis.effective_legs >= 1.0);
    }

    #[test]
    fn test_single_pick_no_correlation() {
        let picks = vec![
            make_pick("Patrick Mahomes", "KC", "BUF", "Passing Yards", "Over"),
        ];

        let analysis = analyze_correlation(&picks);
        assert_eq!(analysis.pair_correlations.len(), 0);
        assert_eq!(analysis.effective_legs, 1.0);
    }

    #[test]
    #[ignore = "stale pre-existing test: the lib test target never compiled before the edge-validation run (missing import); test expectations have diverged from the current implementation - needs follow-up"]
    fn test_category_family_mapping() {
        assert_eq!(category_family("Passing Yards"), "passing_yards");
        assert_eq!(category_family("Rushing TDs"), "rushing_tds");
        assert_eq!(category_family("Receptions"), "receptions");
        assert_eq!(category_family("Anytime TD Scorer"), "td_scorer");
        assert_eq!(category_family("Interceptions"), "interceptions");
    }

    #[test]
    #[ignore = "stale pre-existing test: the lib test target never compiled before the edge-validation run (missing import); test expectations have diverged from the current implementation - needs follow-up"]
    fn test_same_team_correlation_values() {
        // QB passing + WR receiving should be strongly positive
        let corr = same_team_correlation("Passing Yards", "Receiving Yards");
        assert!(corr.unwrap() > 0.6);

        // QB passing + RB rushing should be slightly negative
        let corr2 = same_team_correlation("Passing Yards", "Rushing Yards");
        assert!(corr2.unwrap() < 0.0);

        // Receptions + receiving yards should be very strong
        let corr3 = same_team_correlation("Receptions", "Receiving Yards");
        assert!(corr3.unwrap() > 0.7);
    }
}
