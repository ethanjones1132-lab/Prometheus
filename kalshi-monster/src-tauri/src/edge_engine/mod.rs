//! Edge engine — the deterministic decision core from the Fincept integration
//! plan (docs/fincept-integration-plan §4, §5.3, §6).
//!
//! This module owns the money-adjacent math and is deliberately free of I/O,
//! LLM calls, and sidecar dependencies:
//!
//! - **Shrinkage** (§4.1): the tradable probability is a log-odds blend of the
//!   model's opinion and the market price. The market is the prior; the model
//!   earns deviation from it via the calibration re-fit of `lambda`.
//! - **Cost model** (§4.2): Kalshi taker fees `0.07·P·(1−P)` plus entry at the
//!   ask (YES) or the NO ask (`1 − yes_bid`). Edges are always *net*.
//! - **Aggregation** (§5.3): weighted log-odds pooling of agent signals with a
//!   disagreement penalty. Same inputs → same output, with per-agent
//!   attribution recorded for the quarterly calibration review.
//! - **Sizing** (§6): binary Kelly `(p−c)/(1−c)`, fractional (default quarter),
//!   then named hard caps applied as explicit minimums — never multiplied
//!   factors. The binding constraint is reported to the UI by name.
//!
//! Numeric notes:
//! - Probabilities are clamped to `[0.01, 0.99]` before any logit (Appendix C).
//! - The plan's §4.4 worked example states `p_final ≈ 0.756`; the exact value
//!   under its own formula is `0.758922` (verified independently). Golden
//!   tests below pin the *exact* math; the verdict (PASS) is unchanged.
//! - Fee rounding: Kalshi rounds the fee up to the next cent **per order**,
//!   not per contract. Edge math therefore uses the unrounded per-contract
//!   fee, and `order_fee` applies ceiling-to-cent on the order total.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod breakers;
pub mod calibration;
pub mod pipeline;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Tunables for the edge pipeline. Constants are config, not code (§10.5,
/// Appendix C): the fee multiplier and thresholds must be adjustable without
/// recompiling when Kalshi's published schedule changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeConfig {
    /// λ in `logit(p_final) = λ·logit(p_model) + (1−λ)·logit(p_market)`.
    /// Starts at 0.25; re-fit from the forecast ledger (§4.1).
    pub shrinkage_lambda: f64,
    /// Minimum net edge (dollars per $1 payout) to consider a trade. Default
    /// 0.05, hard floor 0.03 (§4.2).
    pub min_edge: f64,
    /// Kalshi taker-fee multiplier (0.07 per published schedule; verify at
    /// implementation time — Appendix C).
    pub fee_multiplier: f64,
    /// Fraction of full Kelly to stake. Default 0.25 (quarter-Kelly, §6.1).
    pub kelly_fraction: f64,
    /// Minimum ensemble confidence for a trade verdict (§4.3 step 7).
    /// Provisional default until the ledger provides data to fit it.
    pub min_confidence: f64,
}

impl Default for EdgeConfig {
    fn default() -> Self {
        Self {
            shrinkage_lambda: 0.25,
            min_edge: 0.05,
            fee_multiplier: 0.07,
            kelly_fraction: 0.25,
            min_confidence: 0.30,
        }
    }
}

/// Hard floor for `min_edge` (§4.2: "never below 0.03").
pub const MIN_EDGE_FLOOR: f64 = 0.03;

impl EdgeConfig {
    /// Returns the effective trade threshold, enforcing the 3¢ floor even if
    /// config was hand-edited below it.
    pub fn effective_min_edge(&self) -> f64 {
        self.min_edge.max(MIN_EDGE_FLOOR)
    }
}

// ---------------------------------------------------------------------------
// Probability math (Appendix C)
// ---------------------------------------------------------------------------

/// Clamp a probability to [0.01, 0.99] so no single overconfident input can
/// dominate a log-odds pool (Appendix C).
pub fn clamp_prob(p: f64) -> f64 {
    p.clamp(0.01, 0.99)
}

/// `logit(p) = ln(p / (1 − p))`. Input is clamped.
pub fn logit(p: f64) -> f64 {
    let p = clamp_prob(p);
    (p / (1.0 - p)).ln()
}

/// Inverse logit.
pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// §4.1 shrinkage: blend model and market in log-odds space. Blending in
/// probability space is wrong at the tails — 0.95 is a much stronger claim
/// than arithmetic averaging admits.
pub fn shrink(p_model: f64, p_market: f64, lambda: f64) -> f64 {
    let lambda = lambda.clamp(0.0, 1.0);
    sigmoid(lambda * logit(p_model) + (1.0 - lambda) * logit(p_market))
}

// ---------------------------------------------------------------------------
// Cost model (§4.2, Appendix C)
// ---------------------------------------------------------------------------

/// Unrounded Kalshi taker fee per contract, in dollars, at price `p` (dollars,
/// 0–1). Used for edge math; rounding applies per order, not per contract.
pub fn fee_per_contract(p: f64, fee_multiplier: f64) -> f64 {
    let p = p.clamp(0.0, 1.0);
    fee_multiplier * p * (1.0 - p)
}

/// Total fee for an order of `contracts` at price `p`, rounded **up** to the
/// next cent per Kalshi's schedule. A tiny epsilon guards against binary
/// floating-point artifacts (e.g. 141.12 cents representing as 141.12000...2)
/// spuriously ceiling to an extra cent.
pub fn order_fee(p: f64, contracts: u32, fee_multiplier: f64) -> f64 {
    let raw = fee_per_contract(p, fee_multiplier) * contracts as f64;
    ((raw * 100.0 - 1e-9).ceil()).max(0.0) / 100.0
}

/// Effective entry cost per $1 payout when buying YES at `ask`.
pub fn entry_cost(ask: f64, fee_multiplier: f64) -> f64 {
    ask + fee_per_contract(ask, fee_multiplier)
}

// ---------------------------------------------------------------------------
// Agent signals and aggregation (§5, §5.3)
// ---------------------------------------------------------------------------

/// A reference to a data input an agent used, with its fetch timestamp —
/// what makes the staleness check in §4.5 enforceable rather than decorative.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRef {
    pub source: String,
    /// ISO-8601 timestamp of when the input was fetched.
    pub fetched_at: String,
}

/// Universal agent output contract (§5). Mirrors the sidecar's Pydantic
/// `AgentSignal`; field names must stay in lock-step with the OpenAPI schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSignal {
    /// e.g. "macro", "news", "sentiment", "technical".
    pub agent: String,
    /// P(YES) in [0.01, 0.99]; `None` means "no opinion" — an agent must
    /// return None rather than a hallucinated number (§5).
    pub probability: Option<f64>,
    /// Self-assessed reliability in [0, 1].
    pub confidence: f64,
    pub rationale: String,
    #[serde(default)]
    pub inputs_used: Vec<DataRef>,
    #[serde(default)]
    pub caveats: Vec<String>,
}

/// One agent's pull on the pooled number — the attribution record that lets
/// the quarterly review discover "the sentiment agent has been pure noise".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContribution {
    pub agent: String,
    pub probability: f64,
    pub confidence: f64,
    /// Effective weight actually used: routing weight × confidence,
    /// normalized across opining agents.
    pub weight_normalized: f64,
}

/// Deterministic ensemble output (§5.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOpinion {
    pub p_model: f64,
    /// Ensemble confidence after the disagreement penalty.
    pub confidence: f64,
    pub contributions: Vec<AgentContribution>,
}

/// Spread (population std-dev of opining probabilities) at which ensemble
/// confidence bottoms out (§5.3: `spread / 0.25`).
const DISAGREEMENT_FULL_PENALTY_SPREAD: f64 = 0.25;

/// §5.3 weighted log-odds pooling with a disagreement penalty.
///
/// Returns `None` when no agent opines or when total effective weight is
/// zero — the plan's sketch divides by `wsum` unguarded; this is the guard.
/// Agents absent from `weights` get weight 0 (excluded, but recorded nowhere:
/// a missing routing weight is a routing-table bug the caller should log).
pub fn aggregate(signals: &[AgentSignal], weights: &HashMap<String, f64>) -> Option<ModelOpinion> {
    let opining: Vec<&AgentSignal> = signals.iter().filter(|s| s.probability.is_some()).collect();
    if opining.is_empty() {
        return None;
    }

    let mut wsum = 0.0;
    let mut pooled_logit = 0.0;
    let mut effective: Vec<(&AgentSignal, f64)> = Vec::with_capacity(opining.len());
    for &s in &opining {
        let routing = weights.get(&s.agent).copied().unwrap_or(0.0);
        let w = routing * s.confidence.clamp(0.0, 1.0);
        if w > 0.0 {
            pooled_logit += w * logit(s.probability.unwrap());
            wsum += w;
            effective.push((s, w));
        }
    }
    if wsum <= 0.0 {
        return None;
    }
    let p_model = sigmoid(pooled_logit / wsum);

    // Disagreement penalty over the agents that actually contributed.
    let probs: Vec<f64> = effective.iter().map(|(s, _)| s.probability.unwrap()).collect();
    let spread = population_std_dev(&probs);
    let mean_conf: f64 = effective
        .iter()
        .map(|(s, _)| s.confidence.clamp(0.0, 1.0))
        .sum::<f64>()
        / effective.len() as f64;
    let confidence = (1.0 - (spread / DISAGREEMENT_FULL_PENALTY_SPREAD).min(1.0)) * mean_conf;

    let contributions = effective
        .iter()
        .map(|(s, w)| AgentContribution {
            agent: s.agent.clone(),
            probability: s.probability.unwrap(),
            confidence: s.confidence,
            weight_normalized: w / wsum,
        })
        .collect();

    Some(ModelOpinion { p_model, confidence, contributions })
}

fn population_std_dev(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64).sqrt()
}

// ---------------------------------------------------------------------------
// Verdict (§4.3 steps 5–7)
// ---------------------------------------------------------------------------

/// Top-of-book quote for a Kalshi binary, YES side, in dollars.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Quote {
    pub yes_bid: f64,
    pub yes_ask: f64,
}

impl Quote {
    pub fn mid(&self) -> f64 {
        (self.yes_bid + self.yes_ask) / 2.0
    }
    /// Price to buy NO: `1 − yes_bid` (selling YES to the bid ≡ buying NO).
    pub fn no_ask(&self) -> f64 {
        1.0 - self.yes_bid
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    TradeYes,
    TradeNo,
    Pass,
}

/// Full output of the deterministic pipeline for one market — everything the
/// forecast-ledger row and the Edge Board card need (§4.3, §7 Phase 0 schema).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeVerdict {
    pub p_market: f64,
    pub p_model: f64,
    pub p_final: f64,
    pub confidence: f64,
    /// Effective cost per $1 payout buying YES (ask + fee).
    pub entry_yes: f64,
    /// Effective cost per $1 payout buying NO (no_ask + fee at no_ask).
    pub entry_no: f64,
    pub edge_net_yes: f64,
    pub edge_net_no: f64,
    pub verdict: Verdict,
    /// Human-readable reasons; PASS entries are calibration data too (§4.3).
    pub reasons: Vec<String>,
}

/// §4.3 steps 5–7: shrinkage → cost model → verdict.
///
/// `flags` carries upstream DO-NOT-TRADE reasons (ambiguous resolution rules,
/// stale inputs, thin book — §4.5). Any flag forces PASS regardless of edge:
/// the #1 loss source in prediction markets is misreading the rules, not
/// mispricing.
pub fn evaluate(opinion: &ModelOpinion, quote: Quote, flags: &[String], cfg: &EdgeConfig) -> EdgeVerdict {
    let p_market = clamp_prob(quote.mid());
    let p_final = shrink(opinion.p_model, p_market, cfg.shrinkage_lambda);

    let entry_yes = entry_cost(quote.yes_ask, cfg.fee_multiplier);
    let entry_no = entry_cost(quote.no_ask(), cfg.fee_multiplier);
    let edge_net_yes = p_final - entry_yes;
    let edge_net_no = (1.0 - p_final) - entry_no;

    let theta = cfg.effective_min_edge();
    let mut reasons: Vec<String> = Vec::new();
    let mut verdict = Verdict::Pass;

    if !flags.is_empty() {
        reasons.push(format!("do-not-trade flags: {}", flags.join("; ")));
    } else if opinion.confidence < cfg.min_confidence {
        reasons.push(format!(
            "ensemble confidence {:.2} below minimum {:.2}",
            opinion.confidence, cfg.min_confidence
        ));
    } else if edge_net_yes >= theta {
        verdict = Verdict::TradeYes;
        reasons.push(format!(
            "net YES edge {:.1}¢ ≥ threshold {:.1}¢",
            edge_net_yes * 100.0,
            theta * 100.0
        ));
    } else if edge_net_no >= theta {
        verdict = Verdict::TradeNo;
        reasons.push(format!(
            "net NO edge {:.1}¢ ≥ threshold {:.1}¢",
            edge_net_no * 100.0,
            theta * 100.0
        ));
    } else {
        reasons.push(format!(
            "best net edge {:.1}¢ below threshold {:.1}¢",
            edge_net_yes.max(edge_net_no) * 100.0,
            theta * 100.0
        ));
    }

    EdgeVerdict {
        p_market,
        p_model: opinion.p_model,
        p_final,
        confidence: opinion.confidence,
        entry_yes,
        entry_no,
        edge_net_yes,
        edge_net_no,
        verdict,
        reasons,
    }
}

// ---------------------------------------------------------------------------
// Sizing: Kelly + named hard caps (§6.1, §6.2, Appendix C)
// ---------------------------------------------------------------------------

/// Full-Kelly fraction for buying a binary at effective cost `c` per $1
/// payout with true probability `p`: `f* = (p − c) / (1 − c)`, floored at 0.
///
/// Sanity anchors (Appendix C): `p = c → 0`; `p = 1 → 1`.
pub fn binary_kelly(p: f64, c: f64) -> f64 {
    if c <= 0.0 || c >= 1.0 {
        return 0.0;
    }
    ((p - c) / (1.0 - c)).max(0.0)
}

/// One named exposure cap (§6.2). `limit_fraction` and `current_fraction`
/// are fractions of bankroll; headroom = limit − current, floored at 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExposureCap {
    /// User-visible name, e.g. "per-position", "per-event", "per-driver:fed-policy".
    pub name: String,
    pub limit_fraction: f64,
    pub current_fraction: f64,
}

impl ExposureCap {
    pub fn headroom(&self) -> f64 {
        (self.limit_fraction - self.current_fraction).max(0.0)
    }
}

/// Default cap set from §6.2. Callers layer per-event / per-day / per-driver
/// caps with live exposure numbers on top; these are the static ones.
pub fn default_caps() -> Vec<ExposureCap> {
    vec![
        ExposureCap { name: "per-position (5%)".into(), limit_fraction: 0.05, current_fraction: 0.0 },
        ExposureCap { name: "legacy app cap (10%)".into(), limit_fraction: 0.10, current_fraction: 0.0 },
    ]
}

/// Result of sizing one candidate trade. The order card shows `bound_by`:
/// "Kelly suggested $X; capped at $Y by per-event limit" (§6.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizedStake {
    /// Dollars to stake (fraction × bankroll), after caps.
    pub stake: f64,
    /// Full-Kelly fraction before the fractional multiplier.
    pub kelly_full: f64,
    /// Fraction of bankroll actually applied.
    pub fraction_applied: f64,
    /// Name of the binding constraint ("kelly" when no cap binds).
    pub bound_by: String,
}

/// §6: one honest Kelly computation, then explicit caps as hard minimums with
/// named reasons — never multiplied haircut factors, which compose into
/// numbers nobody can interpret.
///
/// `p` must be the *shrunk* probability `p_final`; sizing from raw `p_model`
/// would double-count self-confidence (§6.1).
pub fn size_stake(p_final: f64, entry: f64, bankroll: f64, caps: &[ExposureCap], cfg: &EdgeConfig) -> SizedStake {
    let kelly_full = binary_kelly(p_final, entry);
    let candidate = cfg.kelly_fraction.clamp(0.0, 1.0) * kelly_full;

    let mut fraction = candidate;
    let mut bound_by = "kelly".to_string();
    for cap in caps {
        let headroom = cap.headroom();
        if headroom < fraction {
            fraction = headroom;
            bound_by = cap.name.clone();
        }
    }

    SizedStake {
        stake: (fraction * bankroll.max(0.0)).max(0.0),
        kelly_full,
        fraction_applied: fraction,
        bound_by,
    }
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

    // ---- fee model (§4.2, Appendix C) ----

    #[test]
    fn fee_matches_published_schedule_examples() {
        // fee(0.72) = 0.07 · 0.72 · 0.28 = 0.014112 (worked example §4.4)
        assert!(approx(fee_per_contract(0.72, 0.07), 0.014112, 1e-9));
        // Maximum at p = 0.50: 1.75¢ per contract, unrounded.
        assert!(approx(fee_per_contract(0.50, 0.07), 0.0175, 1e-9));
    }

    #[test]
    fn fee_is_symmetric_and_bounded() {
        for i in 1..100 {
            let p = i as f64 / 100.0;
            let f = fee_per_contract(p, 0.07);
            assert!(approx(f, fee_per_contract(1.0 - p, 0.07), 1e-12));
            assert!((0.0..=0.0175 + 1e-12).contains(&f));
        }
    }

    #[test]
    fn order_fee_rounds_up_to_cent_on_the_total() {
        // 100 contracts at 72¢: raw 141.12¢ → 142¢ = $1.42
        assert!(approx(order_fee(0.72, 100, 0.07), 1.42, 1e-9));
        // 1 contract at 50¢: raw 1.75¢ → 2¢
        assert!(approx(order_fee(0.50, 1, 0.07), 0.02, 1e-9));
        // Exact-cent totals must NOT round up an extra cent (fp guard):
        // 10000 contracts at 50¢: raw exactly $175.00
        assert!(approx(order_fee(0.50, 10_000, 0.07), 175.00, 1e-9));
        assert!(approx(order_fee(0.72, 0, 0.07), 0.0, 1e-12));
    }

    // ---- shrinkage (§4.1) ----

    #[test]
    fn shrinkage_worked_example_exact() {
        // §4.4: λ=0.25, p_model=0.87, p_market=0.71.
        // Plan text says ≈0.756; exact value is 0.758922 (independently
        // verified). Pin the exact math.
        let p = shrink(0.87, 0.71, 0.25);
        assert!(approx(p, 0.758922, 1e-5), "got {p}");
    }

    #[test]
    fn shrinkage_endpoints_and_betweenness() {
        assert!(approx(shrink(0.87, 0.71, 0.0), 0.71, 1e-12));
        assert!(approx(shrink(0.87, 0.71, 1.0), 0.87, 1e-12));
        // p_final always lies between market and model (sigmoid is monotone).
        for lam in [0.1, 0.25, 0.5, 0.9] {
            let p = shrink(0.87, 0.71, lam);
            assert!(p > 0.71 && p < 0.87);
        }
        // Model below market: betweenness on the other side.
        let p = shrink(0.30, 0.60, 0.25);
        assert!(p > 0.30 && p < 0.60);
    }

    // ---- aggregation (§5.3) ----

    fn sig(agent: &str, p: Option<f64>, conf: f64) -> AgentSignal {
        AgentSignal {
            agent: agent.into(),
            probability: p,
            confidence: conf,
            rationale: String::new(),
            inputs_used: vec![],
            caveats: vec![],
        }
    }

    fn econ_weights() -> HashMap<String, f64> {
        // §5.2 economic-release routing row.
        [("macro", 0.50), ("news", 0.20), ("technical", 0.20), ("sentiment", 0.10)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    #[test]
    fn aggregation_reproduces_worked_example_p_model() {
        // §4.4 step 3/4: macro .91, news .85, sentiment .80, technical .78
        // (unit confidences) → p_model ≈ 0.8694 ("~0.87" in the plan).
        let signals = vec![
            sig("macro", Some(0.91), 1.0),
            sig("news", Some(0.85), 1.0),
            sig("sentiment", Some(0.80), 1.0),
            sig("technical", Some(0.78), 1.0),
        ];
        let out = aggregate(&signals, &econ_weights()).unwrap();
        assert!(approx(out.p_model, 0.8694, 5e-4), "got {}", out.p_model);
        // Attribution sums to 1 over opining agents.
        let wsum: f64 = out.contributions.iter().map(|c| c.weight_normalized).sum();
        assert!(approx(wsum, 1.0, 1e-9));
    }

    #[test]
    fn aggregation_single_agent_is_identity() {
        let out = aggregate(&[sig("macro", Some(0.65), 0.8)], &econ_weights()).unwrap();
        assert!(approx(out.p_model, 0.65, 1e-9));
        // No disagreement with one voice: penalty factor 1 → conf = mean conf.
        assert!(approx(out.confidence, 0.8, 1e-9));
    }

    #[test]
    fn aggregation_excludes_non_opining_and_zero_weight_agents() {
        let signals = vec![
            sig("macro", Some(0.70), 1.0),
            sig("news", None, 0.9),              // no opinion → excluded
            sig("valuation", Some(0.99), 1.0),   // not in routing table → weight 0
        ];
        let out = aggregate(&signals, &econ_weights()).unwrap();
        assert!(approx(out.p_model, 0.70, 1e-9));
        assert_eq!(out.contributions.len(), 1);
    }

    #[test]
    fn aggregation_none_when_nobody_opines() {
        assert!(aggregate(&[sig("macro", None, 1.0)], &econ_weights()).is_none());
        assert!(aggregate(&[], &econ_weights()).is_none());
        // All weights zero (routing bug) must not divide by zero.
        assert!(aggregate(&[sig("unknown", Some(0.5), 1.0)], &econ_weights()).is_none());
    }

    #[test]
    fn disagreement_penalty_lowers_confidence() {
        let agree = aggregate(
            &[sig("macro", Some(0.80), 1.0), sig("news", Some(0.80), 1.0)],
            &econ_weights(),
        )
        .unwrap();
        let disagree = aggregate(
            &[sig("macro", Some(0.95), 1.0), sig("news", Some(0.45), 1.0)],
            &econ_weights(),
        )
        .unwrap();
        assert!(approx(agree.confidence, 1.0, 1e-9));
        assert!(disagree.confidence < agree.confidence);
        // Spread 0.25 (pop sd) ≥ full-penalty spread → confidence 0.
        assert!(approx(disagree.confidence, 0.0, 1e-9));
    }

    #[test]
    fn aggregation_output_within_input_range() {
        let signals = vec![
            sig("macro", Some(0.91), 0.7),
            sig("news", Some(0.85), 0.4),
            sig("technical", Some(0.78), 0.9),
        ];
        let out = aggregate(&signals, &econ_weights()).unwrap();
        assert!(out.p_model >= 0.78 && out.p_model <= 0.91);
    }

    // ---- verdict: the §4.4 worked example end-to-end ----

    #[test]
    fn worked_example_end_to_end_passes_correctly() {
        // "Will July CPI YoY exceed 3.0%?" — YES ask 72¢, bid 70¢.
        let signals = vec![
            sig("macro", Some(0.91), 1.0),
            sig("news", Some(0.85), 1.0),
            sig("sentiment", Some(0.80), 1.0),
            sig("technical", Some(0.78), 1.0),
        ];
        let opinion = aggregate(&signals, &econ_weights()).unwrap();
        let quote = Quote { yes_bid: 0.70, yes_ask: 0.72 };
        let v = evaluate(&opinion, quote, &[], &EdgeConfig::default());

        assert!(approx(v.p_market, 0.71, 1e-12));
        // With exact p_model 0.8694: p_final ≈ 0.75867.
        assert!(approx(v.p_final, 0.75867, 5e-4), "got {}", v.p_final);
        assert!(approx(v.entry_yes, 0.734112, 1e-6));
        // Net edge ≈ +2.5¢ — real, but below the 5¢ threshold. The raw
        // "87% vs 72¢" view would have screamed +15%; the honest number
        // PASSes, and that is the system working (§4.4).
        assert!(v.edge_net_yes > 0.02 && v.edge_net_yes < 0.03);
        assert_eq!(v.verdict, Verdict::Pass);
    }

    #[test]
    fn verdict_trades_no_side_when_model_is_below_market() {
        // Market says 30¢ YES; ensemble is far more bearish.
        let signals = vec![sig("macro", Some(0.05), 1.0), sig("news", Some(0.08), 1.0)];
        let opinion = aggregate(&signals, &econ_weights()).unwrap();
        let quote = Quote { yes_bid: 0.30, yes_ask: 0.32 };
        let v = evaluate(&opinion, quote, &[], &EdgeConfig::default());
        // no_ask = 1 − 0.30 = 0.70; P(NO) after shrinkage must clear
        // 0.70 + fee(0.70) + 5¢.
        assert!(approx(v.entry_no, 0.70 + 0.0147, 1e-6));
        assert_eq!(v.verdict, Verdict::TradeNo);
        assert!(v.edge_net_no >= EdgeConfig::default().effective_min_edge());
    }

    #[test]
    fn do_not_trade_flags_force_pass_regardless_of_edge() {
        let signals = vec![sig("macro", Some(0.99), 1.0)];
        let opinion = aggregate(&signals, &econ_weights()).unwrap();
        let quote = Quote { yes_bid: 0.40, yes_ask: 0.42 }; // huge apparent edge
        let flags = vec!["ambiguous resolution criterion".to_string()];
        let v = evaluate(&opinion, quote, &flags, &EdgeConfig::default());
        assert_eq!(v.verdict, Verdict::Pass);
        assert!(v.reasons[0].contains("do-not-trade"));
    }

    #[test]
    fn low_ensemble_confidence_forces_pass() {
        let signals = vec![
            sig("macro", Some(0.95), 1.0),
            sig("news", Some(0.50), 1.0), // strong disagreement → confidence ≈ 0
        ];
        let opinion = aggregate(&signals, &econ_weights()).unwrap();
        let quote = Quote { yes_bid: 0.40, yes_ask: 0.42 };
        let v = evaluate(&opinion, quote, &[], &EdgeConfig::default());
        assert_eq!(v.verdict, Verdict::Pass);
        assert!(v.reasons[0].contains("confidence"));
    }

    #[test]
    fn min_edge_floor_is_enforced() {
        let cfg = EdgeConfig { min_edge: 0.001, ..EdgeConfig::default() }; // hand-edited below the floor
        assert!(approx(cfg.effective_min_edge(), MIN_EDGE_FLOOR, 1e-12));
    }

    // ---- Kelly + caps (§6, Appendix C) ----

    #[test]
    fn kelly_sanity_anchors() {
        // p = c → no edge, no bet.
        assert!(approx(binary_kelly(0.60, 0.60), 0.0, 1e-12));
        // p = 1 → full bankroll (and exactly why raw Kelly is never used).
        assert!(approx(binary_kelly(1.0, 0.70), 1.0, 1e-12));
        // Negative edge floors at zero.
        assert!(approx(binary_kelly(0.50, 0.60), 0.0, 1e-12));
        // Degenerate cost inputs are safe.
        assert!(approx(binary_kelly(0.5, 1.0), 0.0, 1e-12));
        assert!(approx(binary_kelly(0.5, -0.1), 0.0, 1e-12));
    }

    #[test]
    fn quarter_kelly_with_caps_reports_binding_constraint() {
        let cfg = EdgeConfig::default(); // kelly_fraction 0.25
        // p_final 0.80 vs entry 0.60: f* = 0.2/0.4 = 0.50; quarter → 0.125.
        // per-position cap 5% binds.
        let caps = vec![ExposureCap {
            name: "per-position (5%)".into(),
            limit_fraction: 0.05,
            current_fraction: 0.0,
        }];
        let s = size_stake(0.80, 0.60, 1000.0, &caps, &cfg);
        assert!(approx(s.kelly_full, 0.50, 1e-12));
        assert!(approx(s.fraction_applied, 0.05, 1e-12));
        assert!(approx(s.stake, 50.0, 1e-9));
        assert_eq!(s.bound_by, "per-position (5%)");
    }

    #[test]
    fn kelly_binds_when_caps_are_loose() {
        let cfg = EdgeConfig::default();
        // Small edge: f* = (0.55−0.50)/0.50 = 0.10; quarter → 0.025 < 5% cap.
        let s = size_stake(0.55, 0.50, 1000.0, &default_caps(), &cfg);
        assert!(approx(s.fraction_applied, 0.025, 1e-12));
        assert_eq!(s.bound_by, "kelly");
        assert!(approx(s.stake, 25.0, 1e-9));
    }

    #[test]
    fn exhausted_cap_headroom_zeroes_the_stake() {
        let cfg = EdgeConfig::default();
        let caps = vec![ExposureCap {
            name: "per-event (8%)".into(),
            limit_fraction: 0.08,
            current_fraction: 0.09, // already over via another position
        }];
        let s = size_stake(0.80, 0.60, 1000.0, &caps, &cfg);
        assert!(approx(s.stake, 0.0, 1e-12));
        assert_eq!(s.bound_by, "per-event (8%)");
    }

    #[test]
    fn tightest_of_multiple_caps_binds() {
        let cfg = EdgeConfig::default();
        let caps = vec![
            ExposureCap { name: "per-category (25%)".into(), limit_fraction: 0.25, current_fraction: 0.0 },
            ExposureCap { name: "per-event (8%)".into(), limit_fraction: 0.08, current_fraction: 0.05 },
            ExposureCap { name: "per-position (5%)".into(), limit_fraction: 0.05, current_fraction: 0.0 },
        ];
        // Candidate 0.125; per-event headroom 0.03 is tightest.
        let s = size_stake(0.80, 0.60, 1000.0, &caps, &cfg);
        assert!(approx(s.fraction_applied, 0.03, 1e-12));
        assert_eq!(s.bound_by, "per-event (8%)");
    }

    // ---- serde round-trip (contract stability with the sidecar) ----

    #[test]
    fn agent_signal_deserializes_from_sidecar_json() {
        let json = r#"{
            "agent": "macro",
            "probability": 0.91,
            "confidence": 0.85,
            "rationale": "nowcast 3.2% with sigma 0.15",
            "inputs_used": [{"source": "econdb:CPI-US", "fetched_at": "2026-07-07T12:00:00Z"}],
            "caveats": ["base effects fading"]
        }"#;
        let s: AgentSignal = serde_json::from_str(json).unwrap();
        assert_eq!(s.agent, "macro");
        assert!(approx(s.probability.unwrap(), 0.91, 1e-12));
        // Optional fields may be omitted entirely.
        let minimal: AgentSignal =
            serde_json::from_str(r#"{"agent":"news","probability":null,"confidence":0.5,"rationale":"no data"}"#)
                .unwrap();
        assert!(minimal.probability.is_none());
        assert!(minimal.inputs_used.is_empty());
    }
}
