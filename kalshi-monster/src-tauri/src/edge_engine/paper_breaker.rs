//! Pure helpers for §6.4 breaker stake scaling on the paper-decision path.
//! Extracted so the paper IPC handler and unit tests share one implementation.

/// Result of applying breaker stake multiplier before bankroll / exposure caps.
#[derive(Debug, Clone, PartialEq)]
pub struct PaperBreakerStakeApply {
    pub adjusted_stake: f64,
    pub multiplier_applied: bool,
    pub thesis_note: Option<String>,
}

/// Scale a base stake by the breaker `stake_multiplier` and build the optional
/// thesis annotation the paper path appends when scaling is active.
pub fn apply_paper_breaker_stake(base_stake: f64, stake_multiplier: f64) -> PaperBreakerStakeApply {
    let multiplier_applied = (stake_multiplier - 1.0).abs() > 1e-9;
    let adjusted_stake = base_stake * stake_multiplier;
    let thesis_note = if multiplier_applied {
        Some(format!(
            "breaker stake_multiplier {:.2} applied (paper path)",
            stake_multiplier
        ))
    } else {
        None
    };
    PaperBreakerStakeApply {
        adjusted_stake,
        multiplier_applied,
        thesis_note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unity_multiplier_leaves_stake_unchanged() {
        let r = apply_paper_breaker_stake(250.0, 1.0);
        assert_eq!(r.adjusted_stake, 250.0);
        assert!(!r.multiplier_applied);
        assert!(r.thesis_note.is_none());
    }

    #[test]
    fn drawdown_scaler_halves_stake() {
        let r = apply_paper_breaker_stake(100.0, 0.5);
        assert_eq!(r.adjusted_stake, 50.0);
        assert!(r.multiplier_applied);
        assert_eq!(
            r.thesis_note.as_deref(),
            Some("breaker stake_multiplier 0.50 applied (paper path)")
        );
    }

    #[test]
    fn zero_base_stake_stays_zero_under_scaling() {
        let r = apply_paper_breaker_stake(0.0, 0.5);
        assert_eq!(r.adjusted_stake, 0.0);
        assert!(r.multiplier_applied);
    }
}