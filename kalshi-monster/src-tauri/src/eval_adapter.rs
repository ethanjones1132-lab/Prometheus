//! Adapter: app prediction records -> `edge_eval::GradedPrediction`.
//!
//! This is the app side of the shared evaluation engine. Rows without a
//! stored win probability or still Pending carry no calibration signal and
//! are skipped.

use crate::predictions::tracker::{PredictionOutcome, PredictionRecord};
use edge_eval::types::{GradedPrediction, Outcome};

/// Decimal odds the app's EV math assumes for a standard -110 prop leg.
pub const DEFAULT_PAYOUT: f64 = 1.909;

/// Convert one record. Returns `None` for Pending rows and rows without a
/// usable probability.
pub fn record_to_graded(r: &PredictionRecord) -> Option<GradedPrediction> {
    let outcome = match r.outcome {
        PredictionOutcome::Pending => return None,
        PredictionOutcome::Win => Outcome::Win,
        PredictionOutcome::Loss => Outcome::Loss,
        PredictionOutcome::Push => Outcome::Push,
        PredictionOutcome::Void => Outcome::Void,
    };
    // Stored as a percent (0-100); see predictions/tracker.rs extraction.
    let prob_pct = r.prediction.probability?;
    if !prob_pct.is_finite() {
        return None;
    }
    let timestamp = chrono::DateTime::parse_from_rfc3339(&r.prediction.created_at)
        .ok()
        .map(|d| d.timestamp());

    let mut g = GradedPrediction::new(
        r.prediction.id.clone(),
        (prob_pct / 100.0).clamp(0.0, 1.0),
        outcome,
    )
    .with_payout(DEFAULT_PAYOUT);
    g.category = r.prediction.stat_category.clone();
    g.confidence_tier = r.prediction.confidence.clone();
    g.timestamp = timestamp;
    Some(g)
}

/// Convert a batch, dropping rows with no calibration signal.
pub fn records_to_graded(records: &[PredictionRecord]) -> Vec<GradedPrediction> {
    records.iter().filter_map(record_to_graded).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::predictions::tracker::Prediction;

    fn record(prob: Option<f64>, outcome: PredictionOutcome) -> PredictionRecord {
        PredictionRecord {
            prediction: Prediction {
                id: "id-1".into(),
                session_id: "s".into(),
                raw_text: String::new(),
                player_name: Some("Player".into()),
                pick_type: Some("Over".into()),
                line: Some(100.5),
                stat_category: Some("Passing Yards".into()),
                confidence: Some("High".into()),
                confidence_score: Some(72),
                probability: prob,
                reasoning: None,
                risk: None,
                created_at: "2026-01-04T18:00:00+00:00".into(),
                full_decision_json: None,
            },
            outcome,
            actual_result: None,
            notes: None,
            resolved_at: None,
        }
    }

    #[test]
    fn maps_percent_probability_and_outcome() {
        let g = record_to_graded(&record(Some(62.0), PredictionOutcome::Win)).unwrap();
        assert!((g.predicted_prob - 0.62).abs() < 1e-12);
        assert_eq!(g.outcome, Outcome::Win);
        assert_eq!(g.category.as_deref(), Some("Passing Yards"));
        assert!(g.timestamp.is_some());
    }

    #[test]
    fn skips_pending_and_missing_probability() {
        assert!(record_to_graded(&record(Some(62.0), PredictionOutcome::Pending)).is_none());
        assert!(record_to_graded(&record(None, PredictionOutcome::Loss)).is_none());
    }

    #[test]
    fn push_and_void_map_through() {
        assert_eq!(
            record_to_graded(&record(Some(55.0), PredictionOutcome::Push))
                .unwrap()
                .outcome,
            Outcome::Push
        );
        assert_eq!(
            record_to_graded(&record(Some(55.0), PredictionOutcome::Void))
                .unwrap()
                .outcome,
            Outcome::Void
        );
    }
}
