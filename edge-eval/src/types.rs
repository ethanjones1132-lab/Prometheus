use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Win,
    Loss,
    Push,
    Void,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradedPrediction {
    pub id: String,
    pub predicted_prob: f64,
    pub outcome: Outcome,
    pub payout: f64,
    pub category: Option<String>,
    pub confidence_tier: Option<String>,
    pub timestamp: Option<i64>,
}

impl GradedPrediction {
    pub fn new(id: String, predicted_prob: f64, outcome: Outcome) -> Self {
        Self {
            id,
            predicted_prob: predicted_prob.clamp(0.0, 1.0),
            outcome,
            payout: 1.909,
            category: None,
            confidence_tier: None,
            timestamp: None,
        }
    }

    pub fn with_payout(mut self, payout: f64) -> Self {
        self.payout = payout;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsotonicCalibrator {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibratorKind {
    #[serde(rename = "Isotonic")]
    pub isotonic: IsotonicCalibrator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calibrator {
    pub kind: CalibratorKind,
    pub n_fit: u64,
    pub clamp_lo: f64,
    pub clamp_hi: f64,
}

impl Calibrator {
    pub fn kind_name(&self) -> &'static str {
        "Isotonic"
    }

    /// Calibrate a probability in 0–1.
    ///
    /// The output is clamped to (0, 1): no finite fitting sample justifies
    /// emitting certainty (0% or 100%) into staking math, and a leaky fit
    /// must never surface "Win Prob: 100%".
    pub fn apply(&self, raw: f64) -> f64 {
        let p = raw.clamp(self.clamp_lo, self.clamp_hi);
        let iso = &self.kind.isotonic;
        interpolate(&iso.xs, &iso.ys, p).clamp(0.01, 0.99)
    }
}

fn interpolate(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    if xs.is_empty() || ys.is_empty() || xs.len() != ys.len() {
        return x;
    }
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[xs.len() - 1] {
        return ys[ys.len() - 1];
    }
    for i in 1..xs.len() {
        if x <= xs[i] {
            let x0 = xs[i - 1];
            let x1 = xs[i];
            let y0 = ys[i - 1];
            let y1 = ys[i];
            let t = if (x1 - x0).abs() < f64::EPSILON {
                0.0
            } else {
                (x - x0) / (x1 - x0)
            };
            return y0 + t * (y1 - y0);
        }
    }
    ys[ys.len() - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal(xs: Vec<f64>, ys: Vec<f64>) -> Calibrator {
        Calibrator {
            kind: CalibratorKind {
                isotonic: IsotonicCalibrator { xs, ys },
            },
            n_fit: 10,
            clamp_lo: 0.01,
            clamp_hi: 0.99,
        }
    }

    #[test]
    fn apply_never_emits_certainty() {
        // A leaky fit mapping to 0.0 / 1.0 must still clamp to (0, 1).
        let c = cal(vec![0.1, 0.5, 0.9], vec![0.0, 0.5, 1.0]);
        assert!(c.apply(0.999) <= 0.99);
        assert!(c.apply(0.9) <= 0.99);
        assert!(c.apply(0.1) >= 0.01);
        assert!(c.apply(0.001) >= 0.01);
    }

    #[test]
    fn apply_identity_artifact_is_noop() {
        let c = cal(vec![0.01, 0.99], vec![0.01, 0.99]);
        assert!((c.apply(0.55) - 0.55).abs() < 1e-9);
        assert!((c.apply(0.5) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn interpolate_mismatched_lengths_returns_input() {
        assert!((interpolate(&[0.1, 0.2], &[0.5], 0.15) - 0.15).abs() < 1e-9);
        assert!((interpolate(&[], &[], 0.42) - 0.42).abs() < 1e-9);
    }

    #[test]
    fn apply_respects_clamp_band_on_input() {
        let c = cal(vec![0.2, 0.8], vec![0.3, 0.7]);
        // Below clamp_lo → input clamps to 0.01 → curve floor ys[0] = 0.3.
        assert!((c.apply(0.0) - 0.3).abs() < 1e-9);
        // Above clamp_hi → input clamps to 0.99 → curve ceiling ys[last] = 0.7.
        assert!((c.apply(1.0) - 0.7).abs() < 1e-9);
    }
}