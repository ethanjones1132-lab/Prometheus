//! Forecast ledger — the canonical record of every prediction-market opinion the
//! system produces, whether it becomes a trade or not.  Every later phase (calibration,
//! edge-engine, portfolio risk, execution) depends on this table.
//!
//! Schema per the Fincept integration plan §7 Phase 0.

use sqlx::Sqlite;
use sqlx::{Pool, Row};
use serde::{Deserialize, Serialize};

// ── Forecast row ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Forecast {
    pub id: i64,
    pub market_ticker: String,
    pub created_at: String,           // ISO 8601
    pub close_time: String,
    pub p_market: f64,                // mid at analysis time
    pub p_model: Option<f64>,         // NULL until Phase 2 agents exist
    pub p_final: f64,
    pub verdict: String,              // 'trade_yes' | 'trade_no' | 'pass'
    pub verdict_reasons: String,      // JSON array of strings
    pub stake_suggested: Option<f64>,
    pub agent_breakdown: Option<String>, // JSON: per-agent p, confidence, weight

    // Resolution columns (filled by the poller after settlement):
    pub resolved_at: Option<String>,
    pub outcome: Option<i32>,         // 1 = YES, 0 = NO
    pub brier_model: Option<f64>,
    pub brier_market: Option<f64>,
    pub brier_final: Option<f64>,
}

// ── Schema initialisation ───────────────────────────────────────────────────

pub async fn init_forecast_table(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS forecasts (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            market_ticker   TEXT NOT NULL,
            created_at      TEXT NOT NULL,
            close_time      TEXT NOT NULL,
            p_market        REAL NOT NULL,
            p_model         REAL,
            p_final         REAL NOT NULL,
            verdict         TEXT NOT NULL,
            verdict_reasons TEXT NOT NULL,
            stake_suggested REAL,
            agent_breakdown TEXT,

            resolved_at     TEXT,
            outcome         INTEGER,
            brier_model     REAL,
            brier_market    REAL,
            brier_final     REAL
        );
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create forecasts table: {e}"))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_fc_ticker ON forecasts(market_ticker);",
    )
    .execute(pool)
    .await
    .ok();

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_fc_verdict ON forecasts(verdict);",
    )
    .execute(pool)
    .await
    .ok();

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_fc_resolved ON forecasts(resolved_at);",
    )
    .execute(pool)
    .await
    .ok();

    Ok(())
}

// ── CRUD ────────────────────────────────────────────────────────────────────

/// Insert a new forecast row.  Returns the auto-incremented id.
pub async fn insert_forecast(
    pool: &Pool<Sqlite>,
    market_ticker: &str,
    created_at: &str,
    close_time: &str,
    p_market: f64,
    p_model: Option<f64>,
    p_final: f64,
    verdict: &str,
    verdict_reasons: &str,
    stake_suggested: Option<f64>,
    agent_breakdown: Option<&str>,
) -> Result<i64, String> {
    let row = sqlx::query(
        r#"
        INSERT INTO forecasts (
            market_ticker, created_at, close_time,
            p_market, p_model, p_final,
            verdict, verdict_reasons,
            stake_suggested, agent_breakdown
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#,
    )
    .bind(market_ticker)
    .bind(created_at)
    .bind(close_time)
    .bind(p_market)
    .bind(p_model)
    .bind(p_final)
    .bind(verdict)
    .bind(verdict_reasons)
    .bind(stake_suggested)
    .bind(agent_breakdown)
    .execute(pool)
    .await
    .map_err(|e| format!("forecast insert: {e}"))?;

    Ok(row.last_insert_rowid())
}

/// Record a resolution outcome on a forecast row.
pub async fn resolve_forecast(
    pool: &Pool<Sqlite>,
    id: i64,
    outcome: i32,          // 1 = YES, 0 = NO
    resolved_at: &str,
) -> Result<(), String> {
    // Compute Brier scores inline so they're atomic with the update.
    let row = sqlx::query(
        "SELECT p_market, p_model, p_final FROM forecasts WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("forecast resolve fetch: {e}"))?
    .ok_or_else(|| format!("forecast {id} not found"))?;

    let p_market: f64 = row.get(0);
    let p_model: Option<f64> = row.get(1);
    let p_final: f64 = row.get(2);

    let outcome_f64 = outcome as f64;
    let brier = |p: f64| (p - outcome_f64).powi(2);

    let brier_market = brier(p_market);
    let brier_model = p_model.map(|p| brier(p));
    let brier_final = brier(p_final);

    sqlx::query(
        r#"
        UPDATE forecasts
        SET resolved_at = ?1,
            outcome     = ?2,
            brier_market = ?3,
            brier_model  = ?4,
            brier_final  = ?5
        WHERE id = ?6
        "#,
    )
    .bind(resolved_at)
    .bind(outcome)
    .bind(brier_market)
    .bind(brier_model)
    .bind(brier_final)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| format!("forecast resolve update: {e}"))?;

    Ok(())
}

/// Return all unresolved forecasts (no outcome yet).
pub async fn unresolved_forecasts(pool: &Pool<Sqlite>) -> Result<Vec<Forecast>, String> {
    let rows = sqlx::query(
        "SELECT id, market_ticker, created_at, close_time, p_market, p_model, p_final, verdict, verdict_reasons, stake_suggested, agent_breakdown, resolved_at, outcome, brier_model, brier_market, brier_final FROM forecasts WHERE outcome IS NULL ORDER BY created_at",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("unresolved forecasts: {e}"))?;

    let forecasts: Vec<Forecast> = rows
        .iter()
        .map(|r| Forecast {
            id: r.get(0),
            market_ticker: r.get(1),
            created_at: r.get(2),
            close_time: r.get(3),
            p_market: r.get(4),
            p_model: r.get(5),
            p_final: r.get(6),
            verdict: r.get(7),
            verdict_reasons: r.get(8),
            stake_suggested: r.get(9),
            agent_breakdown: r.get(10),
            resolved_at: r.get(11),
            outcome: r.get(12),
            brier_model: r.get(13),
            brier_market: r.get(14),
            brier_final: r.get(15),
        })
        .collect();

    Ok(forecasts)
}

/// Resolve every unresolved forecast row for a market that has settled.
pub async fn resolve_forecasts_for_market(
    pool: &Pool<Sqlite>,
    market_ticker: &str,
    actual: &str,
    resolved_at: &str,
) -> Result<u32, String> {
    let outcome = match actual {
        "Yes" => 1,
        "No" => 0,
        _ => return Ok(0),
    };

    let rows = sqlx::query("SELECT id FROM forecasts WHERE market_ticker = ?1 AND outcome IS NULL")
        .bind(market_ticker)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("forecast ids for ticker: {e}"))?;

    let mut count = 0u32;
    for row in rows {
        let id: i64 = row.get(0);
        resolve_forecast(pool, id, outcome, resolved_at).await?;
        count += 1;
    }
    Ok(count)
}

/// Resolved rows reduced to what the Phase 3 calibration math consumes
/// (`edge_engine::calibration`), sorted **ascending by resolution time** —
/// the ordering contract `rolling_degradation` requires.
pub async fn resolved_forecasts_for_calibration(
    pool: &Pool<Sqlite>,
) -> Result<Vec<crate::edge_engine::calibration::ResolvedForecast>, String> {
    let rows = sqlx::query(
        "SELECT p_market, p_model, p_final, outcome FROM forecasts \
         WHERE outcome IS NOT NULL ORDER BY resolved_at ASC, id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("resolved forecasts for calibration: {e}"))?;

    Ok(rows
        .iter()
        .map(|r| crate::edge_engine::calibration::ResolvedForecast {
            p_market: r.get(0),
            p_model: r.get(1),
            p_final: r.get(2),
            outcome: r.get::<i64, _>(3) == 1,
        })
        .collect())
}

/// Count of resolved forecasts (for the calibration gate).
pub async fn resolved_count(pool: &Pool<Sqlite>) -> Result<i64, String> {
    let row = sqlx::query("SELECT COUNT(*) FROM forecasts WHERE outcome IS NOT NULL")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("resolved count: {e}"))?;
    Ok(row.get::<i64, _>(0))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn mem_pool() -> Pool<Sqlite> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init_forecast_table(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn insert_and_read() {
        let pool = mem_pool().await;
        let id = insert_forecast(
            &pool,
            "KXTEST",
            "2026-07-07T00:00:00Z",
            "2026-08-01T00:00:00Z",
            0.72,
            None,           // no agent yet
            0.72,
            "pass",
            r#"["edge below threshold"]"#,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(id > 0);
    }

    #[tokio::test]
    async fn resolve_computes_brier() {
        let pool = mem_pool().await;
        let id = insert_forecast(
            &pool,
            "KXTEST",
            "2026-07-01T00:00:00Z",
            "2026-08-01T00:00:00Z",
            0.70,           // market said 70%
            Some(0.80),     // model said 80%
            0.72,           // shrunk final
            "trade_yes",
            r#"["strong edge"]"#,
            Some(50.0),
            Some(r#"{"macro": {"p":0.80, "w":0.5}}"#),
        )
        .await
        .unwrap();

        resolve_forecast(&pool, id, 1, "2026-08-02T00:00:00Z").await.unwrap();

        let unresolved = unresolved_forecasts(&pool).await.unwrap();
        assert!(unresolved.is_empty());

        let count = resolved_count(&pool).await.unwrap();
        assert_eq!(count, 1);

        // Verify Brier scores via raw query
        let row = sqlx::query("SELECT brier_market, brier_model, brier_final FROM forecasts WHERE id = ?1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        // market 70%, outcome YES(1) → (0.70-1)^2 = 0.09
        let bm: f64 = row.get(0);
        assert!((bm - 0.09).abs() < 0.001, "brier_market={bm}");
        // model 80%, outcome YES(1) → (0.80-1)^2 = 0.04
        let bmod: f64 = row.get(1);
        assert!((bmod - 0.04).abs() < 0.001, "brier_model={bmod}");
        // final 72%, outcome YES(1) → (0.72-1)^2 = 0.0784
        let bf: f64 = row.get(2);
        assert!((bf - 0.0784).abs() < 0.001, "brier_final={bf}");
    }

    #[tokio::test]
    async fn no_outcome_on_pass() {
        let pool = mem_pool().await;
        let id = insert_forecast(
            &pool,
            "KXTEST",
            "2026-07-01T00:00:00Z",
            "2026-08-01T00:00:00Z",
            0.50,
            None,
            0.50,
            "pass",
            r#"[]"#,
            None,
            None,
        )
        .await
        .unwrap();

        resolve_forecast(&pool, id, 0, "2026-08-02T00:00:00Z").await.unwrap();

        let count = resolved_count(&pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn calibration_accessor_orders_by_resolution_time() {
        let pool = mem_pool().await;
        // Insert two rows and resolve them out of insertion order.
        let a = insert_forecast(
            &pool, "KXA", "2026-07-01T00:00:00Z", "2026-08-01T00:00:00Z",
            0.60, Some(0.80), 0.65, "trade_yes", r#"[]"#, Some(10.0), None,
        )
        .await
        .unwrap();
        let b = insert_forecast(
            &pool, "KXB", "2026-07-02T00:00:00Z", "2026-08-01T00:00:00Z",
            0.40, None, 0.40, "pass", r#"[]"#, None, None,
        )
        .await
        .unwrap();

        // b resolves first, a later — accessor must return [b, a].
        resolve_forecast(&pool, b, 0, "2026-08-02T00:00:00Z").await.unwrap();
        resolve_forecast(&pool, a, 1, "2026-08-03T00:00:00Z").await.unwrap();

        let rows = resolved_forecasts_for_calibration(&pool).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!((rows[0].p_market - 0.40).abs() < 1e-12);
        assert!(!rows[0].outcome);
        assert!(rows[0].p_model.is_none());
        assert!((rows[1].p_market - 0.60).abs() < 1e-12);
        assert!(rows[1].outcome);
        assert!((rows[1].p_model.unwrap() - 0.80).abs() < 1e-12);
    }

    #[tokio::test]
    async fn resolve_all_for_ticker() {
        let pool = mem_pool().await;
        let _ = insert_forecast(
            &pool,
            "KXMULTI",
            "2026-07-01T00:00:00Z",
            "2026-08-01T00:00:00Z",
            0.60,
            None,
            0.65,
            "trade_yes",
            r#"[]"#,
            Some(10.0),
            None,
        )
        .await
        .unwrap();
        let _ = insert_forecast(
            &pool,
            "KXMULTI",
            "2026-07-02T00:00:00Z",
            "2026-08-01T00:00:00Z",
            0.55,
            None,
            0.55,
            "pass",
            r#"[]"#,
            None,
            None,
        )
        .await
        .unwrap();

        let n = resolve_forecasts_for_market(&pool, "KXMULTI", "Yes", "2026-08-02T00:00:00Z")
            .await
            .unwrap();
        assert_eq!(n, 2);
        assert!(unresolved_forecasts(&pool).await.unwrap().is_empty());
    }
}