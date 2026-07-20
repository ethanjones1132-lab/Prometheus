//! Phase 3 calibration core (plan §7 Phase 3, §4.1 re-fit, §11).
//!
//! Everything here is deterministic, I/O-free math over resolved forecast
//! ledger rows. The DB accessor lives in `kalshi::forecast`; the dashboard and
//! re-fit job wire these functions to SQLite and the UI.
//!
//! Design decisions worth stating:
//!
//! - **λ re-fit is an argmin over mean Brier of the *re-shrunk* probability**,
//!   not over the stored `p_final` (which was produced under the old λ). Each
//!   candidate λ re-blends the ledger's `p_model`/`p_market` pairs and scores
//!   the result against outcomes — exactly the plan's "λ is chosen to minimize
//!   Brier score of p_final over resolved forecasts" (§4.1).
//! - **Ties break toward smaller λ.** When the ledger can't distinguish two
//!   λ values, humility wins: lean on the market.
//! - **Rows without `p_model` are excluded from the re-fit** (there is nothing
//!   to blend) **and from the gate**. On such a row `p_final == p_market` by
//!   construction, so including it in the gate's Brier(p_final) vs
//!   Brier(p_market) comparison satisfies that condition by identity rather
//!   than by skill — 258 of the live ledger's 338 rows are of this kind. They
//!   remain honest evidence about *market* calibration, which is what
//!   [`BrierSummary`] over the raw slice reports; they are simply not evidence
//!   about the model. See [`eligible_rows`].
//! - **The rolling degradation check returns `None` below its window** rather
//!   than a partial-window verdict. A circuit breaker must not trip — or be
//!   declared healthy — on insufficient data (§6.4).

use serde::{Deserialize, Serialize};

use super::{clamp_prob, shrink};

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// One resolved ledger row, reduced to what calibration math needs.
///
/// Ordering contract: functions that are recency-sensitive
/// ([`rolling_degradation`]) document that they take rows **sorted ascending
/// by resolution time** and use the tail of the slice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedForecast {
    pub p_market: f64,
    /// `None` for rows written before agents existed (Phase 0/1 ledger rows)
    /// and for market-only sample-build rows where `p_final == p_market` by
    /// construction. Such a row cannot demonstrate model skill.
    pub p_model: Option<f64>,
    pub p_final: f64,
    /// `true` = market resolved YES.
    pub outcome: bool,
    /// The underlying event this row belongs to (`kalshi::ticker::event_key`).
    /// Ten strike legs of one baseball game share one key. `None` on rows
    /// written before the provenance columns existed and never backfilled —
    /// treated as their own event, since correlation cannot be proven.
    #[serde(default)]
    pub event_key: Option<String>,
    /// `true` when the row was written at or after the underlying event began,
    /// so the quote it recorded already contained the outcome in progress.
    #[serde(default)]
    pub is_in_play: bool,
}

impl ResolvedForecast {
    fn y(&self) -> f64 {
        if self.outcome { 1.0 } else { 0.0 }
    }
}

/// Brier score of a single probability against a binary outcome.
pub fn brier(p: f64, outcome: bool) -> f64 {
    let y = if outcome { 1.0 } else { 0.0 };
    (p - y).powi(2)
}

// ---------------------------------------------------------------------------
// Brier summary (dashboard header numbers)
// ---------------------------------------------------------------------------

/// Mean Brier scores over a set of resolved forecasts.
///
/// `brier_model` is averaged only over rows that carry a `p_model`
/// (`n_model` of them); comparing it against `brier_market` restricted to the
/// same rows is the honest model-vs-market comparison, so both restricted
/// means are reported alongside the full-sample market/final means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrierSummary {
    /// Total resolved rows.
    pub n: usize,
    /// Rows with a model opinion.
    pub n_model: usize,
    /// Mean Brier of the market mid, all rows.
    pub brier_market: f64,
    /// Mean Brier of the shrunk tradable probability, all rows.
    pub brier_final: f64,
    /// Mean Brier of the raw model, rows with `p_model` only. `None` if none.
    pub brier_model: Option<f64>,
    /// Mean Brier of the market restricted to rows with `p_model` —
    /// the apples-to-apples opponent for `brier_model`.
    pub brier_market_on_model_rows: Option<f64>,
}

/// Compute the [`BrierSummary`]. Returns `None` on an empty slice — a mean
/// over nothing is a bug upstream, not a zero.
pub fn brier_summary(rows: &[ResolvedForecast]) -> Option<BrierSummary> {
    if rows.is_empty() {
        return None;
    }
    let n = rows.len();
    let mut sum_market = 0.0;
    let mut sum_final = 0.0;
    let mut sum_model = 0.0;
    let mut sum_market_on_model = 0.0;
    let mut n_model = 0usize;

    for r in rows {
        sum_market += brier(r.p_market, r.outcome);
        sum_final += brier(r.p_final, r.outcome);
        if let Some(pm) = r.p_model {
            sum_model += brier(pm, r.outcome);
            sum_market_on_model += brier(r.p_market, r.outcome);
            n_model += 1;
        }
    }

    Some(BrierSummary {
        n,
        n_model,
        brier_market: sum_market / n as f64,
        brier_final: sum_final / n as f64,
        brier_model: (n_model > 0).then(|| sum_model / n_model as f64),
        brier_market_on_model_rows: (n_model > 0).then(|| sum_market_on_model / n_model as f64),
    })
}

// ---------------------------------------------------------------------------
// Reliability diagram (§7 Phase 3 dashboard)
// ---------------------------------------------------------------------------

/// One bucket of the reliability diagram: forecasts binned by predicted
/// probability; calibrated forecasts have `observed_frequency ≈
/// mean_predicted` in every populated bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReliabilityBucket {
    /// Inclusive lower edge.
    pub lower: f64,
    /// Exclusive upper edge (inclusive for the last bucket, so p = 1.0 lands
    /// in bucket 9 rather than a phantom bucket 10).
    pub upper: f64,
    pub n: usize,
    /// Mean predicted probability of forecasts in the bucket; 0.0 when empty.
    pub mean_predicted: f64,
    /// Fraction of bucket forecasts that resolved YES; 0.0 when empty.
    pub observed_frequency: f64,
}

/// Bin `(probability, outcome)` pairs into `n_buckets` equal-width buckets
/// over [0, 1]. The caller chooses which probability to diagram (p_final for
/// the headline dashboard; p_model / p_market for diagnosis).
pub fn reliability_diagram(pairs: &[(f64, bool)], n_buckets: usize) -> Vec<ReliabilityBucket> {
    let n_buckets = n_buckets.max(1);
    let width = 1.0 / n_buckets as f64;
    let mut counts = vec![0usize; n_buckets];
    let mut sum_p = vec![0.0f64; n_buckets];
    let mut sum_y = vec![0.0f64; n_buckets];

    for &(p, outcome) in pairs {
        let p = p.clamp(0.0, 1.0);
        // p = 1.0 belongs to the top bucket, not index n_buckets.
        let idx = ((p / width) as usize).min(n_buckets - 1);
        counts[idx] += 1;
        sum_p[idx] += p;
        sum_y[idx] += if outcome { 1.0 } else { 0.0 };
    }

    (0..n_buckets)
        .map(|i| ReliabilityBucket {
            lower: i as f64 * width,
            upper: (i + 1) as f64 * width,
            n: counts[i],
            mean_predicted: if counts[i] > 0 { sum_p[i] / counts[i] as f64 } else { 0.0 },
            observed_frequency: if counts[i] > 0 { sum_y[i] / counts[i] as f64 } else { 0.0 },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// λ re-fit (§4.1)
// ---------------------------------------------------------------------------

/// Minimum rows with `p_model` before a re-fit is attempted. Below this the
/// argmin is noise and the current λ stands (plan §13 item 5: cold start).
pub const LAMBDA_REFIT_MIN_SAMPLES: usize = 50;

/// Grid resolution for the λ search. The objective is smooth and
/// one-dimensional on [0, 1]; a 0.001 grid (1001 evaluations) is exact enough
/// that any residual error is far below estimation noise, and it is fully
/// deterministic — no line-search tolerance to bikeshed.
const LAMBDA_GRID_STEPS: usize = 1000;

/// Result of a λ re-fit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LambdaFit {
    /// The argmin λ ∈ [0, 1].
    pub lambda: f64,
    /// Mean Brier of the re-shrunk probability at the fitted λ.
    pub brier_at_fit: f64,
    /// Mean Brier at λ = 0 (pure market) — the baseline to beat.
    pub brier_at_market: f64,
    /// Mean Brier at λ = 1 (pure model) — reported for the review doc.
    pub brier_at_model: f64,
    /// Rows used (those carrying `p_model`).
    pub n: usize,
}

/// Re-fit λ from resolved forecasts by minimizing the mean Brier score of
/// `shrink(p_model, p_market, λ)` against outcomes (§4.1).
///
/// Returns `None` when fewer than `min_samples` rows carry a model opinion.
/// If the model is junk, the argmin collapses toward 0 and the system
/// correctly stops finding "edge" — that is the design, not a failure mode.
pub fn refit_lambda(rows: &[ResolvedForecast], min_samples: usize) -> Option<LambdaFit> {
    let usable: Vec<(f64, f64, f64)> = rows
        .iter()
        .filter_map(|r| r.p_model.map(|pm| (clamp_prob(pm), clamp_prob(r.p_market), r.y())))
        .collect();
    if usable.len() < min_samples.max(1) {
        return None;
    }

    let mean_brier_at = |lambda: f64| -> f64 {
        usable
            .iter()
            .map(|&(pm, pmkt, y)| {
                let p = shrink(pm, pmkt, lambda);
                (p - y).powi(2)
            })
            .sum::<f64>()
            / usable.len() as f64
    };

    let mut best_lambda = 0.0;
    let mut best_brier = f64::INFINITY;
    for step in 0..=LAMBDA_GRID_STEPS {
        let lambda = step as f64 / LAMBDA_GRID_STEPS as f64;
        let b = mean_brier_at(lambda);
        // Strict `<`: ties keep the smaller λ (humility — lean on the market).
        if b < best_brier {
            best_brier = b;
            best_lambda = lambda;
        }
    }

    Some(LambdaFit {
        lambda: best_lambda,
        brier_at_fit: best_brier,
        brier_at_market: mean_brier_at(0.0),
        brier_at_model: mean_brier_at(1.0),
        n: usable.len(),
    })
}

// ---------------------------------------------------------------------------
// The calibration gate (§7 Phase 3 — "the gate, in code")
// ---------------------------------------------------------------------------

/// Gate thresholds. Config, not code (§10.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateConfig {
    /// Minimum resolved forecasts before the gate can open. Plan default 200.
    pub min_resolved: usize,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self { min_resolved: 200 }
    }
}

/// Rows that can actually testify to *model* skill, in ledger order.
///
/// **Ordering contract:** `rows` must be sorted **ascending by resolution
/// time** — the order [`crate::kalshi::forecast::resolved_forecasts_for_calibration`]
/// returns. The dedup below keeps the first row it sees per event, so which
/// leg survives is determined by the caller's `ORDER BY`; an unordered slice
/// makes the choice non-deterministic rather than merely arbitrary.
///
/// Three filters, each removing a class of row that inflates the sample count
/// without adding evidence:
///
/// 1. **`p_model` must exist.** A row where `p_final == p_market` by
///    construction (the market-only sample-build path) measures the market
///    against itself. It is legitimate data about *market* calibration and
///    stays in the ledger — it just cannot be counted as model evidence.
/// 2. **Not in-play.** A forecast written after the event started copies a
///    price that already contains the score. Scoring it is scoring hindsight.
/// 3. **One row per `event_key`.** Ten strike legs of one baseball game move
///    together; they are one observation of the world, not ten. The first row
///    of each event survives (ledger order is ascending by resolution time, so
///    this is the earliest-resolving leg — a deterministic choice, not the
///    best-scoring one).
pub fn eligible_rows(rows: &[ResolvedForecast]) -> Vec<&ResolvedForecast> {
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for r in rows {
        if r.p_model.is_none() || r.is_in_play {
            continue;
        }
        match r.event_key.as_deref() {
            // An unknown event key cannot be proven correlated with anything,
            // so the row stands alone rather than being silently merged.
            None => out.push(r),
            Some(key) => {
                if seen.insert(key) {
                    out.push(r);
                }
            }
        }
    }
    out
}

/// The gate verdict with every condition reported individually, so the
/// dashboard can show *which* requirement is unmet rather than a bare "no".
///
/// Two counts are reported deliberately. `resolved_count` is every resolved
/// row in the ledger — the number that was previously (and misleadingly)
/// checked against `min_resolved`. `eligible_count` is the honest one, and is
/// what the gate actually tests. Showing both makes the gap visible instead of
/// letting a large raw count imply evidence that isn't there.
/// Both Brier views of the ledger, nested so neither is reachable by accident.
///
/// The previous shape had `brier_final` (raw) sitting beside
/// `brier_final_eligible`, and the unqualified name — the misleading one — was
/// what every consumer reached for. Nesting removes the default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateBriers {
    /// Every resolved row. Context only: on market-only rows `p_final ==
    /// p_market`, so any comparison drawn from this is satisfied by identity
    /// rather than by skill. Never render a "beats market" verdict from it.
    pub raw: Option<BrierSummary>,
    /// The eligible rows — model-bearing, pre-event, one per event. This is
    /// the honest model-vs-market comparison and what the gate tests.
    pub eligible: Option<BrierSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateReport {
    pub passed: bool,
    /// Every resolved row, including market-only and in-play rows. Reported
    /// for transparency; **not** what the gate tests.
    pub resolved_count: usize,
    /// Model-bearing, pre-event, one-per-event rows. This is the sample the
    /// gate tests against `min_resolved`.
    pub eligible_count: usize,
    pub min_resolved: usize,
    pub briers: GateBriers,
    pub paper_pnl_after_fees: f64,
    /// Human-readable status of each condition, met or not.
    pub conditions: Vec<String>,
}

/// Evaluate the Phase 3 calibration gate: live execution requires
/// ≥ `min_resolved` **eligible** resolved forecasts AND
/// Brier(p_final) ≤ Brier(p_market) over those eligible rows AND paper P&L
/// after fees > 0 over the window.
///
/// "Eligible" is [`eligible_rows`]: model-bearing, pre-event, deduplicated by
/// event. The raw resolved count is still reported, but gating on it would
/// mean a script that logs `p_final = p_market` on a thousand correlated legs
/// could open live execution without the model ever being tested.
///
/// This function only *evaluates*; persisting `calibration_gate_passed` (and
/// the §6.5 invariant that Phase 5 code checks it) is the caller's job.
pub fn evaluate_gate(
    rows: &[ResolvedForecast],
    paper_pnl_after_fees: f64,
    cfg: &GateConfig,
) -> GateReport {
    let summary = brier_summary(rows);
    let resolved_count = rows.len();

    let eligible: Vec<ResolvedForecast> =
        eligible_rows(rows).into_iter().cloned().collect();
    let eligible_count = eligible.len();
    let eligible_summary = brier_summary(&eligible);

    let count_ok = eligible_count >= cfg.min_resolved;
    let brier_ok = eligible_summary
        .as_ref()
        .is_some_and(|s| s.brier_final <= s.brier_market);
    let pnl_ok = paper_pnl_after_fees > 0.0;

    let conditions = vec![
        format!(
            "{} eligible forecasts ≥ {} required: {} \
             (of {} resolved; excludes rows with no p_model, rows created \
             after the event started, and duplicate legs of one event)",
            eligible_count,
            cfg.min_resolved,
            if count_ok { "met" } else { "NOT met" },
            resolved_count,
        ),
        match &eligible_summary {
            Some(s) => format!(
                "Brier(p_final) {:.4} ≤ Brier(p_market) {:.4} over {} eligible rows: {}",
                s.brier_final,
                s.brier_market,
                eligible_count,
                if brier_ok { "met" } else { "NOT met" }
            ),
            None => "Brier comparison: no eligible forecasts yet (NOT met)".to_string(),
        },
        format!(
            "paper P&L after fees {:.2} > 0: {}",
            paper_pnl_after_fees,
            if pnl_ok { "met" } else { "NOT met" }
        ),
    ];

    GateReport {
        passed: count_ok && brier_ok && pnl_ok,
        resolved_count,
        eligible_count,
        min_resolved: cfg.min_resolved,
        briers: GateBriers { raw: summary, eligible: eligible_summary },
        paper_pnl_after_fees,
        conditions,
    }
}

// ---------------------------------------------------------------------------
// Rolling calibration degradation (§6.4, last row)
// ---------------------------------------------------------------------------

/// Result of the rolling-window model-vs-market check that feeds the
/// calibration-degradation circuit breaker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DegradationCheck {
    pub window: usize,
    pub brier_final: f64,
    pub brier_market: f64,
    /// `brier_final − brier_market`; degradation when this exceeds the margin.
    pub excess: f64,
    pub degraded: bool,
}

/// §6.4: "rolling-50 Brier worse than market's by > 0.02 → revert to paper".
///
/// `rows` must be sorted **ascending by resolution time**; the check uses the
/// most recent `window` rows. Returns `None` when fewer than `window` rows
/// exist — a breaker must not trip (or be declared healthy) on a partial
/// window.
pub fn rolling_degradation(
    rows: &[ResolvedForecast],
    window: usize,
    margin: f64,
) -> Option<DegradationCheck> {
    let window = window.max(1);
    if rows.len() < window {
        return None;
    }
    let tail = &rows[rows.len() - window..];
    let s = brier_summary(tail)?;
    let excess = s.brier_final - s.brier_market;
    Some(DegradationCheck {
        window,
        brier_final: s.brier_final,
        brier_market: s.brier_market,
        excess,
        degraded: excess > margin,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    /// A row that is eligible by default: model-bearing rows get a unique
    /// event key so the dedup filter is a no-op unless a test asks for it.
    fn row(p_market: f64, p_model: Option<f64>, p_final: f64, outcome: bool) -> ResolvedForecast {
        ResolvedForecast {
            p_market,
            p_model,
            p_final,
            outcome,
            event_key: None,
            is_in_play: false,
        }
    }

    fn with_event(mut r: ResolvedForecast, key: &str) -> ResolvedForecast {
        r.event_key = Some(key.to_string());
        r
    }

    fn in_play(mut r: ResolvedForecast) -> ResolvedForecast {
        r.is_in_play = true;
        r
    }

    // ---- brier ----

    #[test]
    fn brier_known_vectors() {
        assert!(approx(brier(0.70, true), 0.09, 1e-12));
        assert!(approx(brier(0.70, false), 0.49, 1e-12));
        assert!(approx(brier(1.0, true), 0.0, 1e-12));
        assert!(approx(brier(0.5, true), 0.25, 1e-12));
        assert!(approx(brier(0.5, false), 0.25, 1e-12));
    }

    // ---- brier_summary ----

    #[test]
    fn summary_handles_missing_p_model_rows() {
        let rows = vec![
            row(0.70, Some(0.80), 0.72, true), // market .09, model .04, final .0784
            row(0.60, None, 0.60, false),      // market .36, final .36
        ];
        let s = brier_summary(&rows).unwrap();
        assert_eq!(s.n, 2);
        assert_eq!(s.n_model, 1);
        assert!(approx(s.brier_market, (0.09 + 0.36) / 2.0, 1e-12));
        assert!(approx(s.brier_final, (0.0784 + 0.36) / 2.0, 1e-12));
        assert!(approx(s.brier_model.unwrap(), 0.04, 1e-12));
        // Apples-to-apples market mean is restricted to the model row.
        assert!(approx(s.brier_market_on_model_rows.unwrap(), 0.09, 1e-12));
    }

    #[test]
    fn summary_none_on_empty() {
        assert!(brier_summary(&[]).is_none());
    }

    // ---- reliability diagram ----

    #[test]
    fn reliability_buckets_bin_and_average_correctly() {
        let pairs = vec![
            (0.05, false),
            (0.08, false),
            (0.72, true),
            (0.78, false),
            (1.0, true), // must land in bucket 9, not a phantom bucket 10
        ];
        let d = reliability_diagram(&pairs, 10);
        assert_eq!(d.len(), 10);
        // Bucket 0: two forecasts, none YES.
        assert_eq!(d[0].n, 2);
        assert!(approx(d[0].mean_predicted, 0.065, 1e-12));
        assert!(approx(d[0].observed_frequency, 0.0, 1e-12));
        // Bucket 7 [0.7, 0.8): two forecasts, one YES.
        assert_eq!(d[7].n, 2);
        assert!(approx(d[7].mean_predicted, 0.75, 1e-12));
        assert!(approx(d[7].observed_frequency, 0.5, 1e-12));
        // Top bucket holds the p = 1.0 forecast.
        assert_eq!(d[9].n, 1);
        assert!(approx(d[9].observed_frequency, 1.0, 1e-12));
        // Empty buckets are present with n = 0.
        assert_eq!(d[3].n, 0);
        let total: usize = d.iter().map(|b| b.n).sum();
        assert_eq!(total, pairs.len());
    }

    #[test]
    fn perfectly_calibrated_data_lies_on_the_diagonal() {
        // 100 forecasts at 0.25 with 25 YES; 100 at 0.75 with 75 YES.
        let mut pairs = Vec::new();
        for i in 0..100 {
            pairs.push((0.25, i < 25));
            pairs.push((0.75, i < 75));
        }
        let d = reliability_diagram(&pairs, 10);
        assert!(approx(d[2].observed_frequency, 0.25, 1e-12));
        assert!(approx(d[7].observed_frequency, 0.75, 1e-12));
    }

    // ---- λ re-fit ----

    /// Rows where the model is systematically right and the market wrong:
    /// the argmin must push λ to the top of the range.
    #[test]
    fn refit_moves_lambda_up_when_model_beats_market() {
        let rows: Vec<ResolvedForecast> =
            (0..60).map(|_| row(0.55, Some(0.95), 0.65, true)).collect();
        let fit = refit_lambda(&rows, LAMBDA_REFIT_MIN_SAMPLES).unwrap();
        assert!(approx(fit.lambda, 1.0, 1e-9), "got λ = {}", fit.lambda);
        assert!(fit.brier_at_fit < fit.brier_at_market);
        assert!(approx(fit.brier_at_model, brier(0.95, true), 1e-12));
        assert_eq!(fit.n, 60);
    }

    /// Rows where the model is anti-informative: λ must collapse to 0 and the
    /// system correctly stops finding "edge" (§4.1).
    #[test]
    fn refit_collapses_lambda_when_model_is_junk() {
        let rows: Vec<ResolvedForecast> =
            (0..60).map(|_| row(0.40, Some(0.90), 0.55, false)).collect();
        let fit = refit_lambda(&rows, LAMBDA_REFIT_MIN_SAMPLES).unwrap();
        assert!(approx(fit.lambda, 0.0, 1e-9), "got λ = {}", fit.lambda);
        assert!(approx(fit.brier_at_fit, fit.brier_at_market, 1e-12));
    }

    /// Mixed evidence produces an interior λ, and the fitted Brier is a true
    /// minimum: no worse than either endpoint.
    #[test]
    fn refit_interior_lambda_beats_both_endpoints() {
        // Model adds signal but is overconfident; market is under-reactive.
        // Alternate outcomes consistent with a true probability of ~0.70 when
        // model says 0.90 and market says 0.60.
        let mut rows = Vec::new();
        for i in 0..100 {
            rows.push(row(0.60, Some(0.90), 0.70, i % 10 < 7)); // 70% YES
        }
        let fit = refit_lambda(&rows, LAMBDA_REFIT_MIN_SAMPLES).unwrap();
        assert!(fit.lambda > 0.0 && fit.lambda < 1.0, "got λ = {}", fit.lambda);
        assert!(fit.brier_at_fit <= fit.brier_at_market + 1e-12);
        assert!(fit.brier_at_fit <= fit.brier_at_model + 1e-12);
        // The optimum blend should sit near the true 0.70: check the
        // re-shrunk probability at the fitted λ.
        let p = shrink(0.90, 0.60, fit.lambda);
        assert!((0.65..=0.75).contains(&p), "re-shrunk p = {p}");
    }

    #[test]
    fn refit_requires_min_samples_and_model_rows() {
        let too_few: Vec<ResolvedForecast> =
            (0..10).map(|_| row(0.5, Some(0.6), 0.52, true)).collect();
        assert!(refit_lambda(&too_few, LAMBDA_REFIT_MIN_SAMPLES).is_none());
        // Plenty of rows, but none carry p_model.
        let no_model: Vec<ResolvedForecast> =
            (0..300).map(|_| row(0.5, None, 0.5, true)).collect();
        assert!(refit_lambda(&no_model, LAMBDA_REFIT_MIN_SAMPLES).is_none());
    }

    // ---- gate ----

    fn calibrated_rows(n: usize) -> Vec<ResolvedForecast> {
        // p_final slightly better than market: market 0.60, final 0.70,
        // outcomes 70% YES.
        (0..n).map(|i| row(0.60, Some(0.80), 0.70, i % 10 < 7)).collect()
    }

    #[test]
    fn gate_passes_only_when_all_three_conditions_hold() {
        let cfg = GateConfig::default();
        let rows = calibrated_rows(200);

        let g = evaluate_gate(&rows, 125.0, &cfg);
        assert!(g.passed, "conditions: {:?}", g.conditions);
        assert_eq!(g.resolved_count, 200);
        assert_eq!(g.eligible_count, 200, "all rows are model-bearing pre-event");
        let e = g.briers.eligible.as_ref().unwrap();
        assert!(e.brier_final <= e.brier_market);

        // Sample size short by one → fail.
        let g = evaluate_gate(&rows[..199], 125.0, &cfg);
        assert!(!g.passed);

        // Negative paper P&L → fail.
        let g = evaluate_gate(&rows, -0.01, &cfg);
        assert!(!g.passed);

        // Zero paper P&L is NOT strictly positive → fail.
        let g = evaluate_gate(&rows, 0.0, &cfg);
        assert!(!g.passed);
    }

    #[test]
    fn gate_fails_when_final_brier_worse_than_market() {
        let cfg = GateConfig::default();
        // Market well-calibrated at 0.70 (70% YES); p_final overconfident at
        // 0.95 — worse Brier.
        let rows: Vec<ResolvedForecast> =
            (0..250).map(|i| row(0.70, Some(0.99), 0.95, i % 10 < 7)).collect();
        let g = evaluate_gate(&rows, 500.0, &cfg);
        assert!(!g.passed);
        let e = g.briers.eligible.as_ref().unwrap();
        assert!(e.brier_final > e.brier_market);
    }

    #[test]
    fn gate_on_empty_ledger_fails_all_brier_conditions() {
        let g = evaluate_gate(&[], 10.0, &GateConfig::default());
        assert!(!g.passed);
        assert!(g.briers.raw.is_none());
        assert!(g.briers.eligible.is_none());
        assert_eq!(g.eligible_count, 0);
    }

    // ---- eligibility filtering (the honest sample) ----

    /// The bug this filter exists to kill: 258 of 338 live ledger rows were
    /// written with `p_final = p_market` by construction. They can never
    /// distinguish model from market, so they must not move the counter.
    #[test]
    fn market_only_rows_never_count_toward_the_gate() {
        let cfg = GateConfig::default();
        // 300 market-only rows: p_model NULL, p_final == p_market.
        let rows: Vec<ResolvedForecast> = (0..300)
            .map(|i| with_event(row(0.60, None, 0.60, i % 10 < 6), &format!("EV{i}")))
            .collect();

        let g = evaluate_gate(&rows, 500.0, &cfg);
        assert_eq!(g.resolved_count, 300, "raw count still reported honestly");
        assert_eq!(g.eligible_count, 0, "none of them can test model skill");
        assert!(!g.passed, "conditions: {:?}", g.conditions);
        // The full-sample Brier comparison is satisfied *by identity* —
        // exactly why the gate must not test it.
        let raw = g.briers.raw.as_ref().unwrap();
        assert!(raw.brier_final <= raw.brier_market);
        assert!(g.briers.eligible.is_none(), "no eligible rows means no honest comparison");
    }

    #[test]
    fn in_play_rows_never_count_toward_the_gate() {
        let cfg = GateConfig { min_resolved: 3 };
        let rows = vec![
            with_event(row(0.60, Some(0.80), 0.70, true), "GAME-A"),
            in_play(with_event(row(0.95, Some(0.99), 0.97, true), "GAME-B")),
            in_play(with_event(row(0.95, Some(0.99), 0.97, true), "GAME-C")),
        ];
        let g = evaluate_gate(&rows, 10.0, &cfg);
        assert_eq!(g.resolved_count, 3);
        assert_eq!(g.eligible_count, 1, "only the pre-event row survives");
        assert!(!g.passed);
    }

    /// Fourteen strike legs of one baseball game are one observation.
    #[test]
    fn correlated_legs_of_one_event_collapse_to_a_single_observation() {
        let rows: Vec<ResolvedForecast> = (0..14)
            .map(|_| {
                with_event(
                    row(0.60, Some(0.80), 0.70, true),
                    "KXMLBTEAMTOTAL-26JUL191215CWSTOR",
                )
            })
            .collect();
        assert_eq!(eligible_rows(&rows).len(), 1);

        // Two distinct games → two observations.
        let mut mixed = rows.clone();
        mixed.push(with_event(row(0.40, Some(0.30), 0.36, false), "KXMLBGAME-26JUL182008LADNYY"));
        assert_eq!(eligible_rows(&mixed).len(), 2);
    }

    #[test]
    fn dedup_keeps_the_first_row_of_each_event() {
        let rows = vec![
            with_event(row(0.60, Some(0.80), 0.70, true), "EV1"),
            with_event(row(0.10, Some(0.20), 0.12, true), "EV1"),
        ];
        let kept = eligible_rows(&rows);
        assert_eq!(kept.len(), 1);
        assert!(approx(kept[0].p_final, 0.70, 1e-12), "earliest leg wins, not the best-scoring one");
    }

    #[test]
    fn rows_without_an_event_key_are_not_silently_merged() {
        // Unknown provenance: correlation cannot be proven, so each row
        // stands alone rather than collapsing to one.
        let rows: Vec<ResolvedForecast> =
            (0..5).map(|_| row(0.60, Some(0.80), 0.70, true)).collect();
        assert_eq!(eligible_rows(&rows).len(), 5);
    }

    /// End-to-end shape of the live ledger as audited: a large raw count made
    /// almost entirely of market-only, in-play and duplicated rows, with a
    /// handful of genuine observations underneath. The gate must stay LOCKED
    /// and must say why.
    #[test]
    fn live_ledger_shape_reports_both_counts_and_stays_locked() {
        let cfg = GateConfig::default();
        let mut rows: Vec<ResolvedForecast> = Vec::new();
        // 258 market-only sample-build rows.
        for i in 0..258 {
            rows.push(with_event(row(0.55, None, 0.55, i % 2 == 0), &format!("MKT{i}")));
        }
        // 14 in-play legs of one game, model-bearing but worthless.
        for _ in 0..14 {
            rows.push(in_play(with_event(
                row(0.90, Some(0.95), 0.92, true),
                "KXMLBTEAMTOTAL-26JUL191215CWSTOR",
            )));
        }
        // 5 correlated pre-event legs of one basketball game → 1 observation.
        for _ in 0..5 {
            rows.push(with_event(
                row(0.88, Some(0.90), 0.89, false),
                "KXNBASUMMERSPREAD-26JUL11MIAORL",
            ));
        }
        // 2 genuine, independent pre-event forecasts.
        rows.push(with_event(row(0.30, Some(0.25), 0.29, false), "KXWNBASPREAD-26JUL09INDPHX"));
        rows.push(with_event(row(0.80, Some(0.86), 0.81, true), "KXITFMATCH-26JUL10LOKALU"));

        let g = evaluate_gate(&rows, 500.0, &cfg);
        assert_eq!(g.resolved_count, 279);
        assert_eq!(g.eligible_count, 3, "one per genuine pre-event event");
        assert!(!g.passed, "279 raw rows must not open the gate");
        assert!(
            g.conditions[0].contains("3 eligible") && g.conditions[0].contains("279 resolved"),
            "both counts must be visible: {}",
            g.conditions[0]
        );
    }

    // ---- rolling degradation ----

    #[test]
    fn degradation_none_below_window() {
        let rows = calibrated_rows(49);
        assert!(rolling_degradation(&rows, 50, 0.02).is_none());
    }

    #[test]
    fn degradation_uses_only_the_most_recent_window() {
        // 50 old rows where the final probability was excellent, then 50
        // recent rows where it is much worse than the market: the check must
        // see only the recent tail and trip.
        let mut rows: Vec<ResolvedForecast> =
            (0..50).map(|_| row(0.60, Some(0.95), 0.95, true)).collect();
        rows.extend((0..50).map(|_| row(0.90, Some(0.20), 0.30, true)));
        let d = rolling_degradation(&rows, 50, 0.02).unwrap();
        assert!(d.degraded, "excess = {}", d.excess);
        assert!(approx(d.brier_market, brier(0.90, true), 1e-12));
        assert!(approx(d.brier_final, brier(0.30, true), 1e-12));
    }

    #[test]
    fn degradation_margin_is_strict() {
        // §6.4 says "worse ... by > 0.02" — strictly. Use dyadic-exact
        // values so `excess == margin` holds bit-for-bit: outcome YES,
        // p_market = 1.0 (Brier 0), p_final = 0.75 (Brier 0.25² = 0.0625,
        // exact in binary floating point) → excess = 0.0625 exactly.
        let rows: Vec<ResolvedForecast> =
            (0..50).map(|_| row(1.0, None, 0.75, true)).collect();
        let at_margin = rolling_degradation(&rows, 50, 0.0625).unwrap();
        assert_eq!(at_margin.excess, 0.0625);
        assert!(!at_margin.degraded, "excess exactly at margin must not trip");
        let past_margin = rolling_degradation(&rows, 50, 0.0624).unwrap();
        assert!(past_margin.degraded);
    }
}
