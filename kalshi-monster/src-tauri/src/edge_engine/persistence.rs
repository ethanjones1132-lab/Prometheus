//! Breaker-state persistence (§6.4 breaker latches, Phase 3 productization).
//!
//! The breaker logic in [`breakers`] is pure but requires `BreakerState` to
//! survive across app restarts — otherwise the 25% drawdown latch, the scaler
//! hysteresis, and the calibration-demotion flag all reset on restart, defeating
//! the latches the plan specifies.
//!
//! This module provides load/save via sqlx against the same SQLite pool the rest
//! of the app uses. The table is tiny (one row) and simple; no migration needed.

use sqlx::{Row, SqlitePool};

use super::breakers::BreakerState;
use super::EdgeConfig;

/// Table name for the single-row breaker state.  Co-located with forecasts
/// in `predictions.db`.
const TABLE: &str = "breaker_state";

const EDGE_TABLE: &str = "edge_config";

// ── Init ──────────────────────────────────────────────────────────────────────

/// Ensure the table exists.  Idempotent — safe to call every startup.
pub async fn init_breaker_table(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(&format!(
        "CREATE TABLE IF NOT EXISTS {TABLE} (
            id               INTEGER PRIMARY KEY CHECK(id = 1),
            stake_scaling    INTEGER NOT NULL DEFAULT 0,
            live_disabled    INTEGER NOT NULL DEFAULT 0,
            paper_forced     INTEGER NOT NULL DEFAULT 0
        );"
    ))
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create {TABLE} table: {e}"))?;

    // Seed row if first run.
    sqlx::query(&format!(
        "INSERT OR IGNORE INTO {TABLE} (id, stake_scaling, live_disabled, paper_forced)
         VALUES (1, 0, 0, 0);"
    ))
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to seed {TABLE} row: {e}"))?;

    Ok(())
}

// ── Load / Save ───────────────────────────────────────────────────────────────

/// Load the persisted breaker latch state.  Returns [`BreakerState::default`]
/// on first run (fresh DB with the seeded row of all zeros).
pub async fn load_breaker_state(pool: &SqlitePool) -> Result<BreakerState, String> {
    let row = sqlx::query(&format!(
        "SELECT stake_scaling, live_disabled, paper_forced FROM {TABLE} WHERE id = 1"
    ))
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to load {TABLE}: {e}"))?;

    match row {
        Some(r) => {
            let stake_scaling: i32 = r.get(0);
            let live_disabled: i32 = r.get(1);
            let paper_forced: i32 = r.get(2);
            Ok(BreakerState {
                stake_scaling_active: stake_scaling != 0,
                live_trading_disabled: live_disabled != 0,
                paper_mode_forced: paper_forced != 0,
            })
        }
        None => Ok(BreakerState::default()),
    }
}

/// Persist the current breaker latch state.  Upserts the single row.
pub async fn save_breaker_state(
    pool: &SqlitePool,
    state: &BreakerState,
) -> Result<(), String> {
    sqlx::query(&format!(
        "INSERT OR REPLACE INTO {TABLE} (id, stake_scaling, live_disabled, paper_forced)
         VALUES (1, {sc}, {ld}, {pf})",
        sc = state.stake_scaling_active as i32,
        ld = state.live_trading_disabled as i32,
        pf = state.paper_mode_forced as i32,
    ))
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to save {TABLE}: {e}"))?;

    Ok(())
}

/// Evaluate §6.4 breakers from paper + forecast ledger inputs and persist latches.
pub async fn evaluate_and_persist_breakers(
    pool: &SqlitePool,
) -> Result<super::breakers::BreakerDecision, String> {
    use super::breakers::{evaluate_breakers, BreakerConfig, BreakerInputs};
    use super::calibration::rolling_degradation;

    let prev = load_breaker_state(pool).await?;
    let daily = crate::paper::daily_realized_loss_fraction(pool).await?;
    let drawdown = crate::paper::current_drawdown_fraction(pool).await?;
    let resolved = crate::kalshi::forecast::resolved_forecasts_for_calibration(pool).await?;
    let cfg = BreakerConfig::default();
    let degradation =
        rolling_degradation(&resolved, cfg.degradation_window, cfg.degradation_margin);
    let inputs = BreakerInputs {
        daily_realized_loss: daily,
        drawdown_from_hwm: drawdown,
        degradation,
    };
    let decision = evaluate_breakers(&prev, &inputs, &cfg);
    save_breaker_state(pool, &decision.state).await?;
    Ok(decision)
}

// ── Edge config (§4.1 persisted λ) ───────────────────────────────────────────

/// Ensure the edge-config table exists. Idempotent — safe every startup.
pub async fn init_edge_config_table(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(&format!(
        "CREATE TABLE IF NOT EXISTS {EDGE_TABLE} (
            id               INTEGER PRIMARY KEY CHECK(id = 1),
            shrinkage_lambda REAL NOT NULL DEFAULT 0.25,
            min_edge         REAL NOT NULL DEFAULT 0.05,
            fee_multiplier   REAL NOT NULL DEFAULT 0.07,
            kelly_fraction   REAL NOT NULL DEFAULT 0.25,
            min_confidence   REAL NOT NULL DEFAULT 0.30
        );"
    ))
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create {EDGE_TABLE} table: {e}"))?;

    sqlx::query(&format!(
        "INSERT OR IGNORE INTO {EDGE_TABLE}
         (id, shrinkage_lambda, min_edge, fee_multiplier, kelly_fraction, min_confidence)
         VALUES (1, 0.25, 0.05, 0.07, 0.25, 0.30);"
    ))
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to seed {EDGE_TABLE} row: {e}"))?;

    Ok(())
}

/// Load persisted edge tunables, merging with [`EdgeConfig::default`] for fields
/// not yet stored in SQLite.
pub async fn load_edge_config(pool: &SqlitePool) -> Result<EdgeConfig, String> {
    init_edge_config_table(pool).await?;
    let row = sqlx::query(&format!(
        "SELECT shrinkage_lambda, min_edge, fee_multiplier, kelly_fraction, min_confidence FROM {EDGE_TABLE} WHERE id = 1"
    ))
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Failed to load {EDGE_TABLE}: {e}"))?;

    let mut cfg = EdgeConfig::default();
    if let Some(r) = row {
        let lambda: f64 = r.get(0);
        cfg.shrinkage_lambda = lambda.clamp(0.0, 1.0);
        let me: f64 = r.get(1);
        if me.is_finite() && me > 0.0 {
            cfg.min_edge = me;
        }
        let fm: f64 = r.get(2);
        if fm.is_finite() && fm > 0.0 {
            cfg.fee_multiplier = fm;
        }
        let kf: f64 = r.get(3);
        if kf.is_finite() && kf > 0.0 {
            cfg.kelly_fraction = kf;
        }
        let mc: f64 = r.get(4);
        if mc.is_finite() && mc >= 0.0 {
            cfg.min_confidence = mc;
        }
    }
    Ok(cfg)
}

/// Persist all edge-engine tunables (plan §4.1, Appendix C).  Only provided
/// (finite) values overwrite defaults; pass 0.0 for values you want to keep as-is.
pub async fn save_edge_config(
    pool: &SqlitePool,
    shrinkage_lambda: f64,
    min_edge: f64,
    fee_multiplier: f64,
    kelly_fraction: f64,
    min_confidence: f64,
) -> Result<EdgeConfig, String> {
    init_edge_config_table(pool).await?;
    let prev = load_edge_config(pool).await.unwrap_or_default();
    let lambda = if shrinkage_lambda.is_finite() {
        shrinkage_lambda.clamp(0.0, 1.0)
    } else {
        prev.shrinkage_lambda
    };
    let me = if min_edge.is_finite() && min_edge > 0.0 {
        min_edge
    } else {
        prev.min_edge
    };
    let fm = if fee_multiplier.is_finite() && fee_multiplier > 0.0 {
        fee_multiplier
    } else {
        prev.fee_multiplier
    };
    let kf = if kelly_fraction.is_finite() && kelly_fraction > 0.0 {
        kelly_fraction
    } else {
        prev.kelly_fraction
    };
    // min_confidence can legitimately be 0, so treat non-positive as "unchanged"
    let mc = if min_confidence.is_finite() && min_confidence > 0.0 {
        min_confidence
    } else {
        prev.min_confidence
    };
    sqlx::query(&format!(
        "INSERT OR REPLACE INTO {EDGE_TABLE}
         (id, shrinkage_lambda, min_edge, fee_multiplier, kelly_fraction, min_confidence)
         VALUES (1, {lambda}, {me}, {fm}, {kf}, {mc})"
    ))
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to save {EDGE_TABLE}: {e}"))?;
    load_edge_config(pool).await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init_breaker_table(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn first_run_defaults_to_clear_state() {
        let pool = fresh_pool().await;
        let state = load_breaker_state(&pool).await.unwrap();
        assert_eq!(state, BreakerState::default());
    }

    #[tokio::test]
    async fn save_and_reload_preserves_all_fields() {
        let pool = fresh_pool().await;
        let s1 = BreakerState {
            stake_scaling_active: true,
            live_trading_disabled: true,
            paper_mode_forced: true,
        };
        save_breaker_state(&pool, &s1).await.unwrap();
        let s2 = load_breaker_state(&pool).await.unwrap();
        assert_eq!(s2, s1);
    }

    #[tokio::test]
    async fn round_trip_partial_state() {
        let pool = fresh_pool().await;
        let s1 = BreakerState {
            stake_scaling_active: true,
            live_trading_disabled: false,
            paper_mode_forced: true,
        };
        save_breaker_state(&pool, &s1).await.unwrap();
        let s2 = load_breaker_state(&pool).await.unwrap();
        assert_eq!(s2, s1);

        // Clear everything.
        save_breaker_state(&pool, &BreakerState::default())
            .await
            .unwrap();
        let s3 = load_breaker_state(&pool).await.unwrap();
        assert_eq!(s3, BreakerState::default());
    }

    #[tokio::test]
    async fn edge_config_defaults_then_persists_all_fields() {
        let pool = fresh_pool().await;
        init_edge_config_table(&pool).await.unwrap();
        let cfg0 = load_edge_config(&pool).await.unwrap();
        assert!(approx(cfg0.shrinkage_lambda, 0.25, 1e-9));
        assert!(approx(cfg0.min_edge, 0.05, 1e-9));
        assert!(approx(cfg0.fee_multiplier, 0.07, 1e-9));
        assert!(approx(cfg0.kelly_fraction, 0.25, 1e-9));
        assert!(approx(cfg0.min_confidence, 0.30, 1e-9));

        // NaN for fields we do not want to change - they keep previous DB values.
        let saved = save_edge_config(&pool, 0.42, 0.08, f64::NAN, f64::NAN, f64::NAN).await.unwrap();
        assert!(approx(saved.shrinkage_lambda, 0.42, 1e-9));
        assert!(approx(saved.min_edge, 0.08, 1e-9));
        assert!(approx(saved.fee_multiplier, 0.07, 1e-9), "unchanged field preserved");
        assert!(approx(saved.kelly_fraction, 0.25, 1e-9), "unchanged field preserved");
        assert!(approx(saved.min_confidence, 0.30, 1e-9), "unchanged field preserved");
        let reloaded = load_edge_config(&pool).await.unwrap();
        assert!(approx(reloaded.shrinkage_lambda, 0.42, 1e-9));
        assert!(approx(reloaded.min_edge, 0.08, 1e-9));
    }

    fn approx(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() <= eps
    }
}