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
    pub fn apply(&self, raw: f64) -> f64 {
        let p = raw.clamp(self.clamp_lo, self.clamp_hi);
        let iso = &self.kind.isotonic;
        interpolate(&iso.xs, &iso.ys, p)
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