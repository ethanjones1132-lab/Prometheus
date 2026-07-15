//! Historical track-record card for the analyst prompt.
//!
//! Surfaces graded prediction + resolved forecast stats so the model can
//! temper confidence. Does **not** alter Kelly/EV formulas — prompt context only.

use serde::Serialize;
use sqlx::{Pool, Row, Sqlite};

/// Minimum resolved samples before we claim a usable calibration signal.
pub const CALIBRATION_CARD_MIN_SAMPLES: usize = 10;

#[derive(Debug, Clone, Serialize, Default)]
pub struct TrackRecordSnapshot {
    pub graded_predictions: usize,
    pub wins: usize,
    pub losses: usize,
    pub pushes: usize,
    pub pending_predictions: usize,
    pub mean_brier_predictions: Option<f64>,
    pub resolved_forecasts: usize,
    pub unresolved_forecasts: usize,
    pub mean_brier_final: Option<f64>,
    pub mean_brier_market: Option<f64>,
    pub mean_brier_model: Option<f64>,
    pub take_verdicts_resolved: usize,
    pub take_hits: usize,
}

impl TrackRecordSnapshot {
    pub fn prediction_win_rate(&self) -> Option<f64> {
        let n = self.wins + self.losses;
        if n == 0 {
            return None;
        }
        Some(self.wins as f64 / n as f64)
    }

    pub fn has_any_signal(&self) -> bool {
        self.graded_predictions > 0 || self.resolved_forecasts > 0 || self.pending_predictions > 0
    }

    /// Prompt block for the analyst. Always safe to inject (short when empty).
    pub fn to_prompt_block(&self) -> String {
        if !self.has_any_signal() {
            return concat!(
                "## MODEL TRACK RECORD (local ledger)\n",
                "- No graded predictions or resolved forecasts yet. Treat fair values as **uncalibrated**.\n",
                "- Prefer PASS/WATCH; use Low confidence unless Live tape + tight book + clear rules.\n",
                "- Do not invent historical edge claims.\n\n",
            )
            .to_string();
        }

        let mut out = String::with_capacity(1024);
        out.push_str("## MODEL TRACK RECORD (local ledger — use for confidence, not as a formula change)\n");

        if self.graded_predictions > 0 {
            let wr = self
                .prediction_win_rate()
                .map(|r| format!("{:.0}%", r * 100.0))
                .unwrap_or_else(|| "n/a".into());
            let brier = self
                .mean_brier_predictions
                .map(|b| format!("{b:.3}"))
                .unwrap_or_else(|| "n/a".into());
            out.push_str(&format!(
                "- Graded picks: {} (W {} / L {} / P {}) | win rate {} | mean Brier {}\n",
                self.graded_predictions, self.wins, self.losses, self.pushes, wr, brier
            ));
        } else {
            out.push_str("- Graded picks: 0 (no Win/Loss rows yet)\n");
        }

        out.push_str(&format!(
            "- Pending prediction rows: {}\n",
            self.pending_predictions
        ));

        if self.resolved_forecasts > 0 {
            let bf = self
                .mean_brier_final
                .map(|b| format!("{b:.3}"))
                .unwrap_or_else(|| "n/a".into());
            let bm = self
                .mean_brier_market
                .map(|b| format!("{b:.3}"))
                .unwrap_or_else(|| "n/a".into());
            let bmod = self
                .mean_brier_model
                .map(|b| format!("{b:.3}"))
                .unwrap_or_else(|| "n/a".into());
            out.push_str(&format!(
                "- Resolved forecasts: {} | Brier final {} / market {} / model {}\n",
                self.resolved_forecasts, bf, bm, bmod
            ));
            if self.take_verdicts_resolved > 0 {
                out.push_str(&format!(
                    "- trade_yes/trade_no resolved: {} (hits where outcome matched side-implied lean: {})\n",
                    self.take_verdicts_resolved, self.take_hits
                ));
            }
        } else {
            out.push_str(&format!(
                "- Resolved forecasts: 0 ({} still open)\n",
                self.unresolved_forecasts
            ));
        }

        let sample = self.graded_predictions.max(self.resolved_forecasts);
        if sample < CALIBRATION_CARD_MIN_SAMPLES {
            out.push_str(&format!(
                "- Sample is **thin** (n={sample} < {CALIBRATION_CARD_MIN_SAMPLES}). Shrink confidence; default PASS when edge is unclear.\n"
            ));
        } else {
            out.push_str(
                "- Sample is usable for confidence tempering. If mean Brier is weak vs market, do not claim High confidence without Live data.\n",
            );
        }

        out.push_str(
            "- Never cite this card as proof of a specific market's fair value — only as a prior on your own calibration.\n\n",
        );
        out
    }
}

/// Load track-record stats from the local SQLite ledger.
pub async fn load_track_record(pool: &Pool<Sqlite>) -> Result<TrackRecordSnapshot, String> {
    let mut snap = TrackRecordSnapshot::default();

    // Predictions outcomes
    let pred_rows = sqlx::query(
        r#"
        SELECT outcome, probability
        FROM predictions
        WHERE outcome IS NOT NULL AND outcome != ''
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("track_record predictions: {e}"))?;

    let mut brier_sum = 0.0;
    let mut brier_n = 0usize;
    for row in &pred_rows {
        let outcome: String = row.get("outcome");
        match outcome.as_str() {
            "Win" => {
                snap.wins += 1;
                snap.graded_predictions += 1;
            }
            "Loss" => {
                snap.losses += 1;
                snap.graded_predictions += 1;
            }
            "Push" => {
                snap.pushes += 1;
                snap.graded_predictions += 1;
            }
            "Pending" => snap.pending_predictions += 1,
            _ => {}
        }
        if matches!(outcome.as_str(), "Win" | "Loss") {
            if let Ok(prob) = row.try_get::<f64, _>("probability") {
                let p = (prob / 100.0).clamp(0.0, 1.0);
                let o = if outcome == "Win" { 1.0 } else { 0.0 };
                brier_sum += (p - o).powi(2);
                brier_n += 1;
            }
        }
    }
    if brier_n > 0 {
        snap.mean_brier_predictions = Some(brier_sum / brier_n as f64);
    }

    // Explicit pending count (outcome Pending or null)
    let pending_row = sqlx::query(
        r#"
        SELECT COUNT(*) as n FROM predictions
        WHERE outcome IS NULL OR outcome = '' OR outcome = 'Pending'
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("track_record pending: {e}"))?;
    snap.pending_predictions = pending_row.get::<i64, _>("n") as usize;

    // Forecast ledger
    let fc_resolved = sqlx::query(
        r#"
        SELECT p_final, p_market, p_model, outcome, verdict
        FROM forecasts
        WHERE outcome IS NOT NULL
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("track_record forecasts resolved: {e}"))?;

    snap.resolved_forecasts = fc_resolved.len();
    let mut sum_f = 0.0;
    let mut sum_m = 0.0;
    let mut sum_mod = 0.0;
    let mut n_mod = 0usize;
    for row in &fc_resolved {
        let outcome: i64 = row.get("outcome");
        let o = outcome as f64;
        let p_final: f64 = row.get("p_final");
        let p_market: f64 = row.get("p_market");
        sum_f += (p_final - o).powi(2);
        sum_m += (p_market - o).powi(2);
        if let Ok(Some(pm)) = row.try_get::<Option<f64>, _>("p_model") {
            sum_mod += (pm - o).powi(2);
            n_mod += 1;
        }
        let verdict: String = row.try_get("verdict").unwrap_or_default();
        if matches!(verdict.as_str(), "trade_yes" | "trade_no") {
            snap.take_verdicts_resolved += 1;
            let hit = (verdict == "trade_yes" && outcome == 1)
                || (verdict == "trade_no" && outcome == 0);
            if hit {
                snap.take_hits += 1;
            }
        }
    }
    if snap.resolved_forecasts > 0 {
        let n = snap.resolved_forecasts as f64;
        snap.mean_brier_final = Some(sum_f / n);
        snap.mean_brier_market = Some(sum_m / n);
        if n_mod > 0 {
            snap.mean_brier_model = Some(sum_mod / n_mod as f64);
        }
    }

    let unres = sqlx::query("SELECT COUNT(*) as n FROM forecasts WHERE outcome IS NULL")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("track_record unresolved: {e}"))?;
    snap.unresolved_forecasts = unres.get::<i64, _>("n") as usize;

    Ok(snap)
}

/// Convenience: load card text or a safe empty-state block on DB errors.
pub async fn track_record_prompt_block(pool: &Pool<Sqlite>) -> String {
    match load_track_record(pool).await {
        Ok(snap) => snap.to_prompt_block(),
        Err(e) => {
            tracing::warn!("track_record load failed: {e}");
            "## MODEL TRACK RECORD\n(unavailable — treat probabilities as uncalibrated)\n\n".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_card_warns_uncalibrated() {
        let s = TrackRecordSnapshot::default();
        let block = s.to_prompt_block();
        assert!(block.contains("uncalibrated"));
        assert!(block.contains("PASS"));
    }

    #[test]
    fn thin_sample_flag() {
        let s = TrackRecordSnapshot {
            graded_predictions: 3,
            wins: 1,
            losses: 2,
            mean_brier_predictions: Some(0.3),
            ..Default::default()
        };
        let block = s.to_prompt_block();
        assert!(block.contains("thin"));
        assert!(block.contains("win rate"));
    }

    #[test]
    fn win_rate_math() {
        let s = TrackRecordSnapshot {
            wins: 3,
            losses: 1,
            graded_predictions: 4,
            ..Default::default()
        };
        assert!((s.prediction_win_rate().unwrap() - 0.75).abs() < 1e-9);
    }
}
