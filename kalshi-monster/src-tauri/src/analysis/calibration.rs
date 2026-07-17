//! Runtime calibrator resolution.
//!
//! Priority: `~/.openclaw/kalshi-monster/calibrator.json` (lets a re-fitted
//! artifact ship without a rebuild) -> the artifact embedded at build time
//! (`analysis/calibrator.json`) -> `None` (no calibration applied).
//!
//! Resolved once per process.

use serde::Serialize;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::analysis::edge_calculator;

/// Calibrator artifact embedded at build time — validated like any override.
fn embedded_calibrator() -> Option<edge_eval::Calibrator> {
    let json = include_str!("calibrator.json");
    let cal: edge_eval::Calibrator = serde_json::from_str(json).ok()?;
    if validate_calibrator(&cal) {
        Some(cal)
    } else {
        log::warn!("calibrator: embedded artifact failed validation");
        None
    }
}

fn override_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".openclaw/kalshi-monster/calibrator.json")
}

/// Parse a calibrator from a JSON file, rejecting malformed or non-isotonic
/// artifacts (knot/curve length mismatch, out-of-range or non-finite values,
/// non-monotonic knots or curve, degenerate clamp band).
pub fn load_from_file(path: &std::path::Path) -> Option<edge_eval::Calibrator> {
    let content = std::fs::read_to_string(path).ok()?;
    let cal: edge_eval::Calibrator = serde_json::from_str(&content).ok()?;
    if validate_calibrator(&cal) {
        Some(cal)
    } else {
        log::warn!("calibrator: rejected invalid artifact at {}", path.display());
        None
    }
}

/// Structural contract for any calibrator artifact (embedded or runtime
/// override): at least 2 knots, everything finite in [0, 1], knots strictly
/// increasing, curve monotonic non-decreasing, valid clamp band.
fn validate_calibrator(cal: &edge_eval::Calibrator) -> bool {
    let iso = &cal.kind.isotonic;
    if iso.xs.len() < 2 || iso.xs.len() != iso.ys.len() {
        return false;
    }
    let unit = |v: &f64| v.is_finite() && *v >= 0.0 && *v <= 1.0;
    if !iso.xs.iter().all(unit) || !iso.ys.iter().all(unit) {
        return false;
    }
    if !cal.clamp_lo.is_finite()
        || !cal.clamp_hi.is_finite()
        || cal.clamp_lo < 0.0
        || cal.clamp_hi > 1.0
        || cal.clamp_lo >= cal.clamp_hi
    {
        return false;
    }
    iso.xs.windows(2).all(|w| w[1] > w[0]) && iso.ys.windows(2).all(|w| w[1] >= w[0])
}

/// The calibrator the app applies to edge_calculator output, if any.
pub fn current() -> Option<&'static edge_eval::Calibrator> {
    static CAL: OnceLock<Option<edge_eval::Calibrator>> = OnceLock::new();
    CAL.get_or_init(|| {
        if let Some(cal) = load_from_file(&override_path()) {
            log::info!(
                "calibrator: runtime override loaded ({}, n_fit={})",
                cal.kind_name(),
                cal.n_fit
            );
            return Some(cal);
        }
        match embedded_calibrator() {
            Some(cal) => {
                log::info!(
                    "calibrator: embedded artifact ({}, n_fit={})",
                    cal.kind_name(),
                    cal.n_fit
                );
                Some(cal.clone())
            }
            None => {
                log::warn!("calibrator: none available, raw probabilities in use");
                None
            }
        }
    })
    .as_ref()
}

/// Result of applying the measured isotonic calibrator to a YES probability.
#[derive(Debug, Clone, Serialize)]
pub struct CalibratedProbability {
    pub raw_pct: f64,
    pub calibrated_pct: f64,
    pub adjustment_pct: f64,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalibrationStatus {
    pub raw_pct: f64,
    pub calibrated_pct: f64,
    pub adjustment_pct: f64,
    pub applied: bool,
    pub artifact_kind: String,
    pub n_fit: usize,
    pub source: String,
    pub volatility_haircut_pct: f64,
    pub category_sample_status: String,
}

/// Apply the shared edge-eval calibrator to a model YES probability (0–100).
pub fn calibrate_yes_probability_pct(raw_pct: f64) -> CalibratedProbability {
    let raw = raw_pct.clamp(0.1, 99.9);
    let mut edge = edge_calculator::calculate_edge(
        "calibration",
        "market",
        50.0,
        raw,
        raw,
        raw,
        None,
        None,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        "Over",
    );
    edge.raw_win_probability = raw;
    edge.win_probability = raw;

    if let Some(cal) = current() {
        edge_calculator::apply_calibration(&mut edge, cal);
        let calibrated = edge.win_probability.clamp(0.1, 99.9);
        CalibratedProbability {
            raw_pct: raw,
            calibrated_pct: calibrated,
            adjustment_pct: calibrated - raw,
            applied: true,
        }
    } else {
        CalibratedProbability {
            raw_pct: raw,
            calibrated_pct: raw,
            adjustment_pct: 0.0,
            applied: false,
        }
    }
}

pub fn calibration_status_for_probability(raw_pct: f64) -> CalibrationStatus {
    let calibrated = calibrate_yes_probability_pct(raw_pct);
    let runtime_override = load_from_file(&override_path());
    let artifact_source = if runtime_override.is_some() {
        "runtime override"
    } else {
        "embedded"
    };
    let artifact = runtime_override.or_else(embedded_calibrator);

    let (artifact_kind, n_fit, source) = if let Some(cal) = artifact.as_ref() {
        (
            cal.kind_name().to_string(),
            cal.n_fit as usize,
            artifact_source.to_string(),
        )
    } else {
        ("none".to_string(), 0, "none".to_string())
    };

    let sample_status = if n_fit >= 1_000 {
        "category-ready"
    } else if n_fit > 0 {
        "shared calibrator"
    } else {
        "raw model probability"
    };

    let volatility_haircut_pct = if calibrated.applied {
        (calibrated.adjustment_pct.abs() * 0.5).clamp(1.0, 12.5)
    } else {
        0.0
    };

    CalibrationStatus {
        raw_pct: calibrated.raw_pct,
        calibrated_pct: calibrated.calibrated_pct,
        adjustment_pct: calibrated.adjustment_pct,
        applied: calibrated.applied,
        artifact_kind,
        n_fit,
        source,
        volatility_haircut_pct,
        category_sample_status: sample_status.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_from_file_rejects_garbage() {
        let dir = std::env::temp_dir();
        let bad = dir.join("km-cal-test-bad.json");
        std::fs::write(&bad, "{not json").unwrap();
        assert!(load_from_file(&bad).is_none());
        assert!(load_from_file(&dir.join("km-cal-test-missing.json")).is_none());
        let _ = std::fs::remove_file(&bad);
    }

    #[test]
    fn load_from_file_rejects_non_monotonic_artifact() {
        let dir = std::env::temp_dir();
        let bad = dir.join("km-cal-test-nonmono.json");
        std::fs::write(
            &bad,
            r#"{"kind":{"Isotonic":{"xs":[0.2,0.8],"ys":[0.9,0.1]}},"n_fit":5,"clamp_lo":0.01,"clamp_hi":0.99}"#,
        )
        .unwrap();
        assert!(load_from_file(&bad).is_none());
        let _ = std::fs::remove_file(&bad);
    }

    #[test]
    fn load_from_file_rejects_out_of_range_curve() {
        let dir = std::env::temp_dir();
        let bad = dir.join("km-cal-test-range.json");
        std::fs::write(
            &bad,
            r#"{"kind":{"Isotonic":{"xs":[0.2,0.8],"ys":[0.2,1.5]}},"n_fit":5,"clamp_lo":0.01,"clamp_hi":0.99}"#,
        )
        .unwrap();
        assert!(load_from_file(&bad).is_none());
        let _ = std::fs::remove_file(&bad);
    }

    #[test]
    fn embedded_artifact_is_valid_and_never_certain() {
        let cal = embedded_calibrator().expect("embedded");
        assert!(cal.apply(0.999) < 1.0);
        assert!(cal.apply(0.001) > 0.0);
        // Identity placeholder: no adjustment until a real refit ships.
        assert_eq!(cal.n_fit, 0);
        assert!((cal.apply(0.55) - 0.55).abs() < 1e-9);
    }

    #[test]
    fn load_from_file_round_trips_embedded() {
        let cal = embedded_calibrator().expect("embedded");
        let path = std::env::temp_dir().join("km-cal-test-good.json");
        std::fs::write(&path, serde_json::to_string(&cal).unwrap()).unwrap();
        let loaded = load_from_file(&path).expect("parse");
        assert_eq!(loaded.n_fit, cal.n_fit);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn status_reports_artifact_and_volatility_haircut() {
        let status = calibration_status_for_probability(60.0);
        assert_eq!(status.raw_pct, 60.0);
        assert!(status.volatility_haircut_pct >= 0.0);
        assert!(!status.source.is_empty());
        assert!(!status.category_sample_status.is_empty());
    }
}
