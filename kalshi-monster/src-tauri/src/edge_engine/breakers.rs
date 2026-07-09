//! Circuit breakers (plan §6.4) — pure Rust, no sidecar dependency.
//!
//! Implemented as a deterministic state machine: `(previous state, inputs) →
//! decision`. All persistence and scheduling live with the caller; this module
//! owns only the transition logic, so every §6.4 row is unit-testable,
//! including the hysteresis and latching behavior the table implies but does
//! not spell out:
//!
//! - **Daily-loss pause is stateless per evaluation.** "New orders paused
//!   until next day" falls out of the caller feeding a day-scoped loss figure;
//!   when the day rolls over, the input resets and the pause lifts.
//! - **The 15% drawdown scaler has hysteresis.** It arms at > 15% and releases
//!   only below 10% ("until drawdown < 10%"). Between 10% and 15% the previous
//!   state persists — without this, an equity curve hovering at the boundary
//!   would flap the multiplier on every tick.
//! - **The 25% breaker latches.** Only [`BreakerState::manual_reenable`]
//!   clears it (§6.4: "requires manual re-enable in Settings"). If drawdown is
//!   still ≥ 25% at the next evaluation, it re-latches immediately.
//! - **Calibration degradation latches until proven healthy.** A full-window
//!   check showing `degraded = false` clears it; `None` (insufficient data)
//!   keeps the previous state — absence of evidence is not recovery.
//!
//! §6.5 invariant #2 ("no order while any circuit breaker is tripped") is
//! expressed by [`BreakerDecision::live_orders_allowed`]; the order path must
//! consult it and there must be a should-fail test that tries to violate it.

use serde::{Deserialize, Serialize};

use super::calibration::DegradationCheck;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// §6.4 thresholds. Config, not code (§10.5); the UI may tighten freely and
/// loosen only past a confirmation dialog (§9).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakerConfig {
    /// Daily realized loss (fraction of bankroll) that pauses new orders.
    pub daily_loss_pause: f64,
    /// Drawdown from high-water mark that arms the stake scaler.
    pub drawdown_scale_arm: f64,
    /// Drawdown below which the stake scaler releases (hysteresis floor).
    pub drawdown_scale_release: f64,
    /// Stake multiplier while the scaler is armed.
    pub scale_multiplier: f64,
    /// Drawdown that disables live trading until manual re-enable.
    pub drawdown_disable: f64,
    /// Rolling window for the calibration-degradation check.
    pub degradation_window: usize,
    /// Excess Brier (final − market) beyond which the model is demoted.
    pub degradation_margin: f64,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            daily_loss_pause: 0.05,
            drawdown_scale_arm: 0.15,
            drawdown_scale_release: 0.10,
            scale_multiplier: 0.5,
            drawdown_disable: 0.25,
            degradation_window: 50,
            degradation_margin: 0.02,
        }
    }
}

// ---------------------------------------------------------------------------
// State and inputs
// ---------------------------------------------------------------------------

/// The latched portion of breaker state — what must survive across
/// evaluations (persisted by the caller alongside config).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreakerState {
    /// 15% drawdown scaler armed (hysteresis latch).
    pub stake_scaling_active: bool,
    /// 25% breaker tripped; cleared only by manual re-enable.
    pub live_trading_disabled: bool,
    /// Calibration degradation demotion to paper mode.
    pub paper_mode_forced: bool,
}

impl BreakerState {
    /// §6.4 row 3: the user re-enables live trading in Settings. Returns the
    /// cleared state; if conditions still warrant it, the next
    /// [`evaluate_breakers`] call re-latches immediately.
    pub fn manual_reenable(&self) -> BreakerState {
        BreakerState { live_trading_disabled: false, ..self.clone() }
    }
}

/// Inputs for one evaluation. All fractions are of current bankroll /
/// high-water mark as noted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakerInputs {
    /// Realized loss **today** as a fraction of start-of-day bankroll.
    /// Positive = loss; a profitable day is ≤ 0.
    pub daily_realized_loss: f64,
    /// `(high_water_mark − equity) / high_water_mark`, floored at 0.
    pub drawdown_from_hwm: f64,
    /// Most recent rolling calibration check, if a full window exists
    /// (`calibration::rolling_degradation`).
    pub degradation: Option<DegradationCheck>,
}

// ---------------------------------------------------------------------------
// Decision
// ---------------------------------------------------------------------------

/// The outcome of one evaluation: the new latched state plus the derived
/// permissions the order path and sizing code consume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BreakerDecision {
    pub state: BreakerState,
    /// §6.5 invariant #2: the live order path must refuse when false.
    /// Paper-mode demotion alone does NOT clear this to true — see
    /// `paper_only`.
    pub live_orders_allowed: bool,
    /// True when analysis/verdicts continue but fills must be simulated
    /// (calibration degradation demotion, §6.4 last row).
    pub paper_only: bool,
    /// Multiplier applied to every stake while the drawdown scaler is armed.
    pub stake_multiplier: f64,
    /// Human-readable reasons for every restriction in force, for the UI and
    /// the notification pipeline. Empty when nothing is tripped.
    pub reasons: Vec<String>,
}

/// Evaluate all §6.4 breakers. Pure: same `(prev, inputs, cfg)` → same
/// decision.
pub fn evaluate_breakers(
    prev: &BreakerState,
    inputs: &BreakerInputs,
    cfg: &BreakerConfig,
) -> BreakerDecision {
    let dd = inputs.drawdown_from_hwm.max(0.0);
    let mut reasons: Vec<String> = Vec::new();

    // Row 1 — daily loss pause (stateless).
    let daily_paused = inputs.daily_realized_loss > cfg.daily_loss_pause;
    if daily_paused {
        reasons.push(format!(
            "daily realized loss {:.1}% > {:.1}% — new orders paused until next day",
            inputs.daily_realized_loss * 100.0,
            cfg.daily_loss_pause * 100.0
        ));
    }

    // Row 2 — drawdown scaler with hysteresis.
    let stake_scaling_active = if prev.stake_scaling_active {
        dd >= cfg.drawdown_scale_release
    } else {
        dd > cfg.drawdown_scale_arm
    };
    if stake_scaling_active {
        reasons.push(format!(
            "drawdown {:.1}% from high-water mark — stakes scaled ×{} until drawdown < {:.0}%",
            dd * 100.0,
            cfg.scale_multiplier,
            cfg.drawdown_scale_release * 100.0
        ));
    }

    // Row 3 — hard disable, latched until manual re-enable.
    let live_trading_disabled = prev.live_trading_disabled || dd > cfg.drawdown_disable;
    if live_trading_disabled {
        reasons.push(format!(
            "drawdown breaker ({:.0}%) tripped — live trading disabled until manually re-enabled",
            cfg.drawdown_disable * 100.0
        ));
    }

    // Row 4 — calibration degradation, latched until a full healthy window.
    let paper_mode_forced = match &inputs.degradation {
        Some(check) => check.degraded,
        None => prev.paper_mode_forced,
    };
    if paper_mode_forced {
        let detail = inputs
            .degradation
            .as_ref()
            .map(|c| {
                format!(
                    " (rolling-{} Brier excess {:.4} > {:.4})",
                    c.window, c.excess, cfg.degradation_margin
                )
            })
            .unwrap_or_default();
        reasons.push(format!(
            "model calibration degraded{detail} — reverted to paper trading"
        ));
    }

    let state = BreakerState { stake_scaling_active, live_trading_disabled, paper_mode_forced };
    BreakerDecision {
        live_orders_allowed: !daily_paused && !live_trading_disabled && !paper_mode_forced,
        paper_only: paper_mode_forced,
        stake_multiplier: if stake_scaling_active { cfg.scale_multiplier } else { 1.0 },
        reasons,
        state,
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

    fn quiet_inputs() -> BreakerInputs {
        BreakerInputs { daily_realized_loss: 0.0, drawdown_from_hwm: 0.0, degradation: None }
    }

    fn healthy_check() -> DegradationCheck {
        DegradationCheck {
            window: 50,
            brier_final: 0.18,
            brier_market: 0.19,
            excess: -0.01,
            degraded: false,
        }
    }

    fn degraded_check() -> DegradationCheck {
        DegradationCheck {
            window: 50,
            brier_final: 0.24,
            brier_market: 0.19,
            excess: 0.05,
            degraded: true,
        }
    }

    #[test]
    fn all_clear_when_nothing_tripped() {
        let d = evaluate_breakers(&BreakerState::default(), &quiet_inputs(), &BreakerConfig::default());
        assert!(d.live_orders_allowed);
        assert!(!d.paper_only);
        assert!(approx(d.stake_multiplier, 1.0, 1e-12));
        assert!(d.reasons.is_empty());
        assert_eq!(d.state, BreakerState::default());
    }

    // ---- row 1: daily loss ----

    #[test]
    fn daily_loss_pause_is_strict_and_stateless() {
        let cfg = BreakerConfig::default();
        // Exactly 5% is NOT a trip ("> 5%").
        let d = evaluate_breakers(
            &BreakerState::default(),
            &BreakerInputs { daily_realized_loss: 0.05, ..quiet_inputs() },
            &cfg,
        );
        assert!(d.live_orders_allowed);

        let d = evaluate_breakers(
            &BreakerState::default(),
            &BreakerInputs { daily_realized_loss: 0.051, ..quiet_inputs() },
            &cfg,
        );
        assert!(!d.live_orders_allowed);
        // Not persisted in state: next day's inputs reset it.
        assert_eq!(d.state, BreakerState::default());
        let next_day = evaluate_breakers(&d.state, &quiet_inputs(), &cfg);
        assert!(next_day.live_orders_allowed);
    }

    #[test]
    fn profitable_day_never_pauses() {
        let d = evaluate_breakers(
            &BreakerState::default(),
            &BreakerInputs { daily_realized_loss: -0.30, ..quiet_inputs() },
            &BreakerConfig::default(),
        );
        assert!(d.live_orders_allowed);
    }

    // ---- row 2: drawdown scaler hysteresis ----

    #[test]
    fn scaler_arms_above_15_and_releases_below_10() {
        let cfg = BreakerConfig::default();
        let s0 = BreakerState::default();

        // 16% → arms, stakes halved, orders still allowed.
        let d1 = evaluate_breakers(
            &s0,
            &BreakerInputs { drawdown_from_hwm: 0.16, ..quiet_inputs() },
            &cfg,
        );
        assert!(d1.state.stake_scaling_active);
        assert!(approx(d1.stake_multiplier, 0.5, 1e-12));
        assert!(d1.live_orders_allowed);

        // Recovers to 12% — inside the hysteresis band, stays armed.
        let d2 = evaluate_breakers(
            &d1.state,
            &BreakerInputs { drawdown_from_hwm: 0.12, ..quiet_inputs() },
            &cfg,
        );
        assert!(d2.state.stake_scaling_active);
        assert!(approx(d2.stake_multiplier, 0.5, 1e-12));

        // 9% — below the release floor, disarms.
        let d3 = evaluate_breakers(
            &d2.state,
            &BreakerInputs { drawdown_from_hwm: 0.09, ..quiet_inputs() },
            &cfg,
        );
        assert!(!d3.state.stake_scaling_active);
        assert!(approx(d3.stake_multiplier, 1.0, 1e-12));
    }

    #[test]
    fn scaler_does_not_arm_inside_band_from_cold() {
        // 12% drawdown with no prior latch: § says "> 15%" arms — the band
        // only *retains* an armed scaler, it never arms one.
        let d = evaluate_breakers(
            &BreakerState::default(),
            &BreakerInputs { drawdown_from_hwm: 0.12, ..quiet_inputs() },
            &BreakerConfig::default(),
        );
        assert!(!d.state.stake_scaling_active);
    }

    // ---- row 3: hard disable latch ----

    #[test]
    fn disable_latches_and_requires_manual_reenable() {
        let cfg = BreakerConfig::default();
        let d1 = evaluate_breakers(
            &BreakerState::default(),
            &BreakerInputs { drawdown_from_hwm: 0.26, ..quiet_inputs() },
            &cfg,
        );
        assert!(d1.state.live_trading_disabled);
        assert!(!d1.live_orders_allowed);
        // Note: 26% also arms the scaler — both rows apply independently.
        assert!(d1.state.stake_scaling_active);

        // Full recovery of equity does NOT clear the latch.
        let d2 = evaluate_breakers(&d1.state, &quiet_inputs(), &cfg);
        assert!(d2.state.live_trading_disabled);
        assert!(!d2.live_orders_allowed);

        // Manual re-enable clears it; healthy inputs keep it clear.
        let cleared = d2.state.manual_reenable();
        let d3 = evaluate_breakers(&cleared, &quiet_inputs(), &cfg);
        assert!(!d3.state.live_trading_disabled);
        assert!(d3.live_orders_allowed);
    }

    #[test]
    fn reenable_while_still_in_drawdown_relatches() {
        let cfg = BreakerConfig::default();
        let tripped = evaluate_breakers(
            &BreakerState::default(),
            &BreakerInputs { drawdown_from_hwm: 0.30, ..quiet_inputs() },
            &cfg,
        );
        let cleared = tripped.state.manual_reenable();
        // Still 30% down at next evaluation → re-latches immediately.
        let d = evaluate_breakers(
            &cleared,
            &BreakerInputs { drawdown_from_hwm: 0.30, ..quiet_inputs() },
            &cfg,
        );
        assert!(d.state.live_trading_disabled);
        assert!(!d.live_orders_allowed);
    }

    // ---- row 4: calibration degradation ----

    #[test]
    fn degradation_demotes_to_paper_and_recovers_on_healthy_window() {
        let cfg = BreakerConfig::default();
        let d1 = evaluate_breakers(
            &BreakerState::default(),
            &BreakerInputs { degradation: Some(degraded_check()), ..quiet_inputs() },
            &cfg,
        );
        assert!(d1.state.paper_mode_forced);
        assert!(d1.paper_only);
        assert!(!d1.live_orders_allowed);

        // Insufficient data (None) is NOT recovery: latch holds.
        let d2 = evaluate_breakers(&d1.state, &quiet_inputs(), &cfg);
        assert!(d2.state.paper_mode_forced);
        assert!(!d2.live_orders_allowed);

        // A full healthy window clears the demotion.
        let d3 = evaluate_breakers(
            &d2.state,
            &BreakerInputs { degradation: Some(healthy_check()), ..quiet_inputs() },
            &cfg,
        );
        assert!(!d3.state.paper_mode_forced);
        assert!(d3.live_orders_allowed);
    }

    // ---- invariant §6.5 #2 ----

    #[test]
    fn any_tripped_breaker_blocks_live_orders() {
        let cfg = BreakerConfig::default();
        let cases: Vec<BreakerInputs> = vec![
            BreakerInputs { daily_realized_loss: 0.06, ..quiet_inputs() },
            BreakerInputs { drawdown_from_hwm: 0.26, ..quiet_inputs() },
            BreakerInputs { degradation: Some(degraded_check()), ..quiet_inputs() },
        ];
        for inputs in cases {
            let d = evaluate_breakers(&BreakerState::default(), &inputs, &cfg);
            assert!(
                !d.live_orders_allowed,
                "breaker failed to block live orders for inputs {inputs:?}"
            );
            assert!(!d.reasons.is_empty());
        }
    }
}
