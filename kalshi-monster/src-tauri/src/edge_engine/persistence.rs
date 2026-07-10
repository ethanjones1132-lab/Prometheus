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

/// Table name for the single-row breaker state.  Co-located with forecasts
/// in `predictions.db`.
const TABLE: &str = "breaker_state";

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
}