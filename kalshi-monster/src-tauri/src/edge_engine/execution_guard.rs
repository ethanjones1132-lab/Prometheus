//! Live order path guard (plan §6.5 invariant #2, Phase 3 productization).
//!
//! The money path must refuse live orders when the calibration gate is locked
//! or any §6.4 breaker blocks [`BreakerDecision::live_orders_allowed`].
//! Phase 5 `place_order` calls [`assert_live_order_allowed`] at entry.

use serde::{Deserialize, Serialize};

use super::breakers::BreakerDecision;

/// Combined eligibility for placing a live Kalshi order (not paper).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveOrderEligibility {
    pub allowed: bool,
    pub calibration_gate_passed: bool,
    pub breakers_live_orders_allowed: bool,
    pub reasons: Vec<String>,
}

/// Pure combine of gate + breaker decision (unit-testable).
pub fn evaluate_live_order_eligibility(
    gate_passed: bool,
    gate_reasons: &[String],
    breakers: &BreakerDecision,
) -> LiveOrderEligibility {
    let mut reasons = Vec::new();
    if !gate_passed {
        reasons.push("calibration gate locked".to_string());
        if !gate_reasons.is_empty() {
            reasons.extend(gate_reasons.iter().cloned());
        }
    }
    if !breakers.live_orders_allowed {
        reasons.push("circuit breaker blocks live orders".to_string());
        reasons.extend(breakers.reasons.iter().cloned());
    }
    let allowed = gate_passed && breakers.live_orders_allowed;
    LiveOrderEligibility {
        allowed,
        calibration_gate_passed: gate_passed,
        breakers_live_orders_allowed: breakers.live_orders_allowed,
        reasons,
    }
}

/// §6.5: order path entry — returns `Err` when live execution must not proceed.
pub fn assert_live_order_allowed(eligibility: &LiveOrderEligibility) -> Result<(), String> {
    if eligibility.allowed {
        return Ok(());
    }
    let msg = if eligibility.reasons.is_empty() {
        "live orders not allowed".to_string()
    } else {
        eligibility.reasons.join("; ")
    };
    Err(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge_engine::breakers::{BreakerConfig, BreakerInputs, BreakerState, evaluate_breakers};

    fn quiet_breakers() -> BreakerDecision {
        evaluate_breakers(
            &BreakerState::default(),
            &BreakerInputs {
                daily_realized_loss: 0.0,
                drawdown_from_hwm: 0.0,
                degradation: None,
            },
            &BreakerConfig::default(),
        )
    }

    #[test]
    fn allows_when_gate_open_and_breakers_clear() {
        let e = evaluate_live_order_eligibility(true, &[], &quiet_breakers());
        assert!(e.allowed);
        assert!(assert_live_order_allowed(&e).is_ok());
    }

    #[test]
    fn blocks_when_gate_locked() {
        let e = evaluate_live_order_eligibility(
            false,
            &["need 200 resolved".to_string()],
            &quiet_breakers(),
        );
        assert!(!e.allowed);
        assert!(assert_live_order_allowed(&e).is_err());
    }

    #[test]
    fn blocks_when_breakers_trip_even_if_gate_open() {
        let cfg = BreakerConfig::default();
        let tripped = evaluate_breakers(
            &BreakerState::default(),
            &BreakerInputs {
                drawdown_from_hwm: 0.26,
                daily_realized_loss: 0.0,
                degradation: None,
            },
            &cfg,
        );
        assert!(!tripped.live_orders_allowed);
        let e = evaluate_live_order_eligibility(true, &[], &tripped);
        assert!(!e.allowed);
        assert!(assert_live_order_allowed(&e).is_err());
    }
}