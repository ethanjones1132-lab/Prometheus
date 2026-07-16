//! Edge Calculator — prop edge scoring for the legacy sports-prop pipeline.
//!
//! Inlined from the former shared `monster-edge-core` crate (since removed).
//! The Kalshi market path uses `edge_engine` instead; this module only feeds
//! the prop-scorer / analysis-context commands.

use edge_eval::Calibrator;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationAdjustment {
    pub raw_pct: f64,
    pub calibrated_pct: f64,
    pub delta_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeFactor {
    pub label: String,
    pub detail: String,
    pub impact: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeScore {
    pub player_name: String,
    pub stat_category: String,
    pub line: f64,
    pub projection: f64,
    pub edge_pct: f64,
    pub win_probability: f64,
    pub expected_value: f64,
    pub kelly_pct: f64,
    pub confidence: String,
    pub confidence_score: u8,
    pub quality_tier: String,
    pub pick_type: String,
    pub factors: Vec<EdgeFactor>,
    pub risks: Vec<String>,
    pub raw_win_probability: f64,
    pub calibration: Option<CalibrationAdjustment>,
}

#[allow(clippy::too_many_arguments)]
pub fn calculate_edge(
    player_name: &str,
    stat_category: &str,
    line: f64,
    projection: f64,
    season_avg: f64,
    last3_avg: f64,
    _home_avg: Option<f64>,
    _away_avg: Option<f64>,
    _is_home: bool,
    _defense_rank: Option<u32>,
    _pace_rank: Option<u32>,
    _usage_rate: Option<f64>,
    _opponent_pace_rank: Option<u32>,
    _park_factor: Option<f64>,
    _goalie_quality_rank: Option<u32>,
    pick_type: &str,
) -> EdgeScore {
    let base = if season_avg > 0.0 {
        season_avg
    } else if last3_avg > 0.0 {
        last3_avg
    } else {
        line
    };
    let delta = projection - line;
    let edge_pct = if line.abs() > f64::EPSILON {
        (delta / line) * 100.0
    } else {
        0.0
    };

    let win_prob = if pick_type.eq_ignore_ascii_case("over") {
        (50.0 + edge_pct * 2.5).clamp(35.0, 75.0)
    } else {
        (50.0 - edge_pct * 2.5).clamp(25.0, 65.0)
    };

    let implied = if base > 0.0 { (projection / base - 1.0) * 100.0 } else { 0.0 };
    let ev = implied * 0.15;
    let kelly = (ev.max(0.0) * 2.0).min(8.0);

    let tier = if ev > 8.0 {
        "Elite"
    } else if ev > 4.0 {
        "Strong"
    } else if ev > 1.0 {
        "Playable"
    } else if ev > -1.0 {
        "Marginal"
    } else {
        "Avoid"
    };

    EdgeScore {
        player_name: player_name.to_string(),
        stat_category: stat_category.to_string(),
        line,
        projection,
        edge_pct,
        win_probability: win_prob,
        expected_value: ev,
        kelly_pct: kelly,
        confidence: if ev > 4.0 {
            "High".into()
        } else if ev > 0.0 {
            "Medium".into()
        } else {
            "Low".into()
        },
        confidence_score: (50.0 + ev * 3.0).clamp(20.0, 90.0) as u8,
        quality_tier: tier.into(),
        pick_type: pick_type.to_string(),
        factors: vec![],
        risks: vec![],
        raw_win_probability: win_prob,
        calibration: None,
    }
}

pub fn apply_calibration(edge: &mut EdgeScore, cal: &Calibrator) {
    let raw = edge.raw_win_probability / 100.0;
    let calibrated = cal.apply(raw) * 100.0;
    edge.calibration = Some(CalibrationAdjustment {
        raw_pct: edge.raw_win_probability,
        calibrated_pct: calibrated,
        delta_pct: calibrated - edge.raw_win_probability,
    });
    edge.win_probability = calibrated;
    edge.factors.push(EdgeFactor {
        label: format!("Calibration ({})", cal.kind_name()),
        detail: format!(
            "n={}: {:.1}% → {:.1}%",
            cal.n_fit, edge.raw_win_probability, calibrated
        ),
        impact: calibrated - edge.raw_win_probability,
    });
}
