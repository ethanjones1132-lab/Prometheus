use crate::types::{GradedPrediction, Outcome};

#[derive(Debug, Clone)]
pub struct ReliabilityBin {
    pub n: u32,
    pub mean_predicted: f64,
    pub empirical: f64,
}

#[derive(Debug, Clone)]
pub struct CalibrationMetricsResult {
    pub n: u32,
    pub brier_score: f64,
    pub brier_skill_score: f64,
    pub reliability: Vec<ReliabilityBin>,
}

pub fn calibration_metrics(
    graded: &[GradedPrediction],
    n_bins: usize,
    _min_per_bin: u32,
    _seed: u64,
) -> CalibrationMetricsResult {
    let scored: Vec<&GradedPrediction> = graded
        .iter()
        .filter(|g| matches!(g.outcome, Outcome::Win | Outcome::Loss))
        .collect();

    if scored.is_empty() {
        return CalibrationMetricsResult {
            n: 0,
            brier_score: 0.0,
            brier_skill_score: 0.0,
            reliability: vec![],
        };
    }

    let n = scored.len() as u32;
    let brier: f64 = scored
        .iter()
        .map(|g| {
            let actual = if g.outcome == Outcome::Win { 1.0 } else { 0.0 };
            let diff = actual - g.predicted_prob;
            diff * diff
        })
        .sum::<f64>()
        / n as f64;

    let mean_out: f64 = scored
        .iter()
        .map(|g| if g.outcome == Outcome::Win { 1.0 } else { 0.0 })
        .sum::<f64>()
        / n as f64;
    let brier_ref = mean_out * (1.0 - mean_out);
    let bss = if brier_ref > f64::EPSILON {
        1.0 - brier / brier_ref
    } else {
        0.0
    };

    let bins = n_bins.max(2);
    let mut reliability = Vec::with_capacity(bins);
    for i in 0..bins {
        let lo = i as f64 / bins as f64;
        let hi = (i + 1) as f64 / bins as f64;
        let bucket: Vec<&&GradedPrediction> = scored
            .iter()
            .filter(|g| g.predicted_prob >= lo && (g.predicted_prob < hi || (i == bins - 1 && g.predicted_prob <= hi)))
            .collect();
        if bucket.is_empty() {
            reliability.push(ReliabilityBin {
                n: 0,
                mean_predicted: (lo + hi) / 2.0,
                empirical: mean_out,
            });
        } else {
            let mean_predicted =
                bucket.iter().map(|g| g.predicted_prob).sum::<f64>() / bucket.len() as f64;
            let empirical = bucket
                .iter()
                .map(|g| if g.outcome == Outcome::Win { 1.0 } else { 0.0 })
                .sum::<f64>()
                / bucket.len() as f64;
            reliability.push(ReliabilityBin {
                n: bucket.len() as u32,
                mean_predicted,
                empirical,
            });
        }
    }

    CalibrationMetricsResult {
        n,
        brier_score: brier,
        brier_skill_score: bss,
        reliability,
    }
}