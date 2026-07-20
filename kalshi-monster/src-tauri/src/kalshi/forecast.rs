//! Forecast ledger — the canonical record of every prediction-market opinion the
//! system produces, whether it becomes a trade or not.  Every later phase (calibration,
//! edge-engine, portfolio risk, execution) depends on this table.
//!
//! Schema per the Fincept integration plan §7 Phase 0.

use sqlx::Sqlite;
use sqlx::{Pool, Row};
use serde::{Deserialize, Serialize};

use super::ticker;

/// Two rows for the same ticker at the same price within this window are one
/// forecast logged twice — a double-submit or an immediate re-run — not two
/// opinions.
///
/// Deliberately narrow. The ledger's 21 existing duplicate tickers sit 0–950s
/// apart at the same price, so only 7 of them fall inside 60s; widening to
/// cover the rest would also collapse genuine re-quotes of a market whose
/// price simply has not moved in sixteen minutes, which is a real second
/// observation. Repeated and correlated rows are neutralised for *measurement*
/// by the dedup in [`crate::edge_engine::calibration::eligible_rows`], which
/// groups by event instead of guessing from a clock. This constant only stops
/// the ledger accumulating more accidental copies.
const DUPLICATE_SUPPRESSION_SECS: i64 = 60;

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

/// Where a forecast row came from and how much of a model was behind it.
///
/// Without this, a row written by a market-only sample-build script is
/// indistinguishable from a row an agent ensemble actually reasoned about —
/// which is how 258 rows of `p_final = p_market` came to be counted as
/// evidence of model skill.
#[derive(Debug, Clone, Copy)]
pub struct ForecastProvenance<'a> {
    /// `"app"`, `"chat"`, or the script filename that wrote the row.
    pub source: &'a str,
    /// How many agents actually returned a probability. `Some(0)` is a
    /// meaningful record ("everyone was silent"); `None` means not measured.
    pub agents_opining: Option<i64>,
}

impl<'a> ForecastProvenance<'a> {
    pub fn from_source(source: &'a str) -> Self {
        Self { source, agents_opining: None }
    }
}

// ── Schema initialisation ───────────────────────────────────────────────────

/// Canonical `forecasts` DDL. Shared with the migration ledger
/// (`predictions::storage` migration 4) so a fresh database and a migrated one
/// converge on exactly the same schema.
pub const FORECASTS_DDL: &str = r#"
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
        brier_final     REAL,

        -- Provenance (migration 4). All nullable: pre-existing rows are
        -- backfilled where derivable and left NULL where they are not.
        event_start_at  TEXT,
        is_in_play      INTEGER DEFAULT 0,
        source          TEXT,
        event_key       TEXT,
        agents_opining  INTEGER
    );
"#;

pub async fn init_forecast_table(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(FORECASTS_DDL)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create forecasts table: {e}"))?;

    // Unlike its siblings below, this one propagates: migration 4 guarantees
    // `event_key` exists before this runs, so a failure here is a real fault,
    // not the legacy-schema case the `.ok()` calls tolerate.
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_fc_event_key ON forecasts(event_key);",
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create idx_fc_event_key: {e}"))?;

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

/// Provenance derived from the ticker and creation time: the event this row
/// belongs to, when that event started, and whether the row was written after
/// the tape went live.
fn derive_provenance(
    market_ticker: &str,
    created_at: &str,
) -> (String, Option<String>, i64) {
    let event_key = ticker::event_key(market_ticker);
    let start = ticker::event_start_from_ticker(market_ticker);
    let in_play = ticker::is_in_play(created_at, start);
    (event_key, start.map(|s| s.to_rfc3339()), i64::from(in_play))
}

const INSERT_SQL: &str = r#"
    INSERT INTO forecasts (
        market_ticker, created_at, close_time,
        p_market, p_model, p_final,
        verdict, verdict_reasons,
        stake_suggested, agent_breakdown,
        event_start_at, is_in_play, source, event_key, agents_opining
    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
"#;

/// SQL for the duplicate guard. Two rows are the same forecast logged twice
/// when the ticker and the market price both match and they land within
/// [`DUPLICATE_SUPPRESSION_SECS`] of each other.
///
/// `julianday` is used rather than string comparison because the ledger holds
/// three different timestamp spellings (`to_rfc3339`, Python `isoformat`, and
/// a millisecond `Z` form) that do not sort against each other reliably.
const DUPLICATE_LOOKUP_SQL: &str = r#"
    SELECT id FROM forecasts
    WHERE market_ticker = ?1
      AND ABS(p_market - ?2) < 1e-9
      AND ABS(julianday(created_at) - julianday(?3)) * 86400.0 <= ?4
    ORDER BY id DESC LIMIT 1
"#;

/// Insert a new forecast row. Returns the auto-incremented id.
///
/// **Duplicate suppression:** if the same ticker already has a row at the same
/// `p_market` within [`DUPLICATE_SUPPRESSION_SECS`], the existing row's id is
/// returned and nothing is written. A caller that double-submits, or re-runs
/// immediately after a crash, gets back the forecast it already logged rather
/// than a second copy that doubles its weight in every count. Returning the
/// existing id rather than an error keeps the guard invisible to well-behaved
/// callers.
///
/// The guard is **advisory, not a constraint**, and is honest about both of
/// its holes:
/// - It is a *read-then-insert*, not atomic. Two concurrent writers can both
///   see no duplicate and both insert. A UNIQUE index would close this, but it
///   would also reject at startup against the 21 duplicate rows already in the
///   live ledger, so it is not on the table.
/// - It **fails open** when SQLite's `julianday()` cannot parse a stored
///   `created_at`: the comparison yields NULL, no row matches, and the insert
///   proceeds. Failing open is the right direction — a missed suppression
///   costs one extra row, a false suppression silently discards a real
///   forecast.
///
/// Existing duplicates are left alone — the gate's dedup-by-event already
/// neutralises them, and deleting logged history is not this function's call.
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
    provenance: ForecastProvenance<'_>,
) -> Result<i64, String> {
    if let Some(existing) = sqlx::query(DUPLICATE_LOOKUP_SQL)
        .bind(market_ticker)
        .bind(p_market)
        .bind(created_at)
        .bind(DUPLICATE_SUPPRESSION_SECS as f64)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("forecast duplicate lookup: {e}"))?
    {
        let id: i64 = existing.get(0);
        tracing::debug!(
            "forecast duplicate suppressed: {market_ticker} p_market={p_market} \
             within {DUPLICATE_SUPPRESSION_SECS}s of forecast#{id}"
        );
        return Ok(id);
    }

    let (event_key, event_start_at, is_in_play) =
        derive_provenance(market_ticker, created_at);

    let row = sqlx::query(INSERT_SQL)
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
        .bind(event_start_at)
        .bind(is_in_play)
        .bind(provenance.source)
        .bind(event_key)
        .bind(provenance.agents_opining)
        .execute(pool)
        .await
        .map_err(|e| format!("forecast insert: {e}"))?;

    Ok(row.last_insert_rowid())
}

/// Transaction-aware version of [`insert_forecast`], including the duplicate
/// guard.
pub async fn insert_forecast_tx(
    txn: &mut sqlx::Transaction<'_, Sqlite>,
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
    provenance: ForecastProvenance<'_>,
) -> Result<i64, String> {
    if let Some(existing) = sqlx::query(DUPLICATE_LOOKUP_SQL)
        .bind(market_ticker)
        .bind(p_market)
        .bind(created_at)
        .bind(DUPLICATE_SUPPRESSION_SECS as f64)
        .fetch_optional(&mut **txn)
        .await
        .map_err(|e| format!("forecast duplicate lookup: {e}"))?
    {
        let id: i64 = existing.get(0);
        tracing::debug!(
            "forecast duplicate suppressed: {market_ticker} p_market={p_market} \
             within {DUPLICATE_SUPPRESSION_SECS}s of forecast#{id}"
        );
        return Ok(id);
    }

    let (event_key, event_start_at, is_in_play) =
        derive_provenance(market_ticker, created_at);

    let row = sqlx::query(INSERT_SQL)
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
        .bind(event_start_at)
        .bind(is_in_play)
        .bind(provenance.source)
        .bind(event_key)
        .bind(provenance.agents_opining)
        .execute(&mut **txn)
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
        "SELECT p_market, p_model, p_final, outcome, event_key, is_in_play FROM forecasts \
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
            event_key: r.get(4),
            is_in_play: r.get::<Option<i64>, _>(5).unwrap_or(0) == 1,
        })
        .collect())
}

/// Count of resolved forecasts, raw — every row with an outcome, including
/// market-only and in-play rows.
///
/// This is a display number. The calibration gate must not be fed from here;
/// use [`resolved_forecasts_for_calibration`] and
/// `edge_engine::calibration::eligible_rows`, which strips the rows that
/// cannot testify to model skill.
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

    fn test_provenance() -> ForecastProvenance<'static> {
        ForecastProvenance { source: "test", agents_opining: Some(2) }
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
            test_provenance(),
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
            test_provenance(),
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
            test_provenance(),
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
            0.60, Some(0.80), 0.65, "trade_yes", r#"[]"#, Some(10.0), None, test_provenance(),
        )
        .await
        .unwrap();
        let b = insert_forecast(
            &pool, "KXB", "2026-07-02T00:00:00Z", "2026-08-01T00:00:00Z",
            0.40, None, 0.40, "pass", r#"[]"#, None, None, test_provenance(),
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
            test_provenance(),
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
            test_provenance(),
        )
        .await
        .unwrap();

        let n = resolve_forecasts_for_market(&pool, "KXMULTI", "Yes", "2026-08-02T00:00:00Z")
            .await
            .unwrap();
        assert_eq!(n, 2);
        assert!(unresolved_forecasts(&pool).await.unwrap().is_empty());
    }

    // ---- provenance ----

    async fn provenance_of(pool: &Pool<Sqlite>, id: i64) -> (String, Option<String>, i64, Option<String>, Option<i64>) {
        let r = sqlx::query(
            "SELECT event_key, event_start_at, is_in_play, source, agents_opining \
             FROM forecasts WHERE id = ?1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
        (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4))
    }

    #[tokio::test]
    async fn insert_records_event_key_and_pre_event_status() {
        let pool = mem_pool().await;
        // First pitch 19:15 ET on 2026-07-17 = 23:15Z; logged three hours before.
        let id = insert_forecast(
            &pool,
            "KXMLBTOTAL-26JUL171915CWSTOR-4",
            "2026-07-17T20:00:00Z",
            "2026-07-18T04:00:00Z",
            0.60,
            Some(0.70),
            0.63,
            "trade_yes",
            r#"[]"#,
            None,
            None,
            ForecastProvenance { source: "app", agents_opining: Some(3) },
        )
        .await
        .unwrap();

        let (key, start, in_play, source, opining) = provenance_of(&pool, id).await;
        assert_eq!(key, "KXMLBTOTAL-26JUL171915CWSTOR");
        assert_eq!(start.as_deref(), Some("2026-07-17T23:15:00+00:00"));
        assert_eq!(in_play, 0, "logged before first pitch");
        assert_eq!(source.as_deref(), Some("app"));
        assert_eq!(opining, Some(3));
    }

    #[tokio::test]
    async fn insert_after_first_pitch_is_flagged_in_play() {
        let pool = mem_pool().await;
        let id = insert_forecast(
            &pool,
            "KXMLBTOTAL-26JUL171915CWSTOR-4",
            "2026-07-18T01:30:00Z", // ~2h into the game
            "2026-07-18T04:00:00Z",
            0.90,
            Some(0.92),
            0.91,
            "trade_yes",
            r#"[]"#,
            None,
            None,
            test_provenance(),
        )
        .await
        .unwrap();
        let (_, _, in_play, _, _) = provenance_of(&pool, id).await;
        assert_eq!(in_play, 1);
    }

    #[tokio::test]
    async fn ticker_without_an_encoded_time_records_unknown_not_pre_event() {
        let pool = mem_pool().await;
        let id = insert_forecast(
            &pool,
            "KXWORLDCUPHALFTIME-26-POS",
            "2026-07-19T02:19:21Z",
            "2026-08-01T00:00:00Z",
            0.30,
            Some(0.29),
            0.30,
            "pass",
            r#"[]"#,
            None,
            None,
            test_provenance(),
        )
        .await
        .unwrap();
        let (key, start, in_play, _, _) = provenance_of(&pool, id).await;
        assert_eq!(key, "KXWORLDCUPHALFTIME-26");
        assert!(start.is_none(), "no time encoded → NULL, not a guess");
        assert_eq!(in_play, 0);
    }

    #[tokio::test]
    async fn calibration_accessor_carries_event_key_and_in_play() {
        let pool = mem_pool().await;
        let id = insert_forecast(
            &pool,
            "KXMLBTOTAL-26JUL171915CWSTOR-4",
            "2026-07-18T01:30:00Z",
            "2026-07-18T04:00:00Z",
            0.90,
            Some(0.92),
            0.91,
            "trade_yes",
            r#"[]"#,
            None,
            None,
            test_provenance(),
        )
        .await
        .unwrap();
        resolve_forecast(&pool, id, 1, "2026-07-18T05:00:00Z").await.unwrap();

        let rows = resolved_forecasts_for_calibration(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event_key.as_deref(), Some("KXMLBTOTAL-26JUL171915CWSTOR"));
        assert!(rows[0].is_in_play);
        // ... and therefore contributes nothing to the honest sample.
        assert!(crate::edge_engine::calibration::eligible_rows(&rows).is_empty());
    }

    // ---- duplicate suppression ----

    async fn row_count(pool: &Pool<Sqlite>) -> i64 {
        sqlx::query("SELECT COUNT(*) FROM forecasts")
            .fetch_one(pool)
            .await
            .unwrap()
            .get(0)
    }

    async fn insert_at(pool: &Pool<Sqlite>, created_at: &str, p_market: f64) -> i64 {
        insert_forecast(
            pool,
            "KXDUP-26JUL191215CWSTOR-TOR8",
            created_at,
            "2026-07-19T20:00:00Z",
            p_market,
            None,
            p_market,
            "pass",
            r#"[]"#,
            None,
            None,
            test_provenance(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn same_ticker_and_price_within_the_window_does_not_create_a_second_row() {
        let pool = mem_pool().await;
        let first = insert_at(&pool, "2026-07-19T10:00:00Z", 0.42).await;
        let second = insert_at(&pool, "2026-07-19T10:00:30Z", 0.42).await;
        assert_eq!(second, first, "caller gets back the row it already logged");
        assert_eq!(row_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn a_genuinely_later_forecast_is_not_suppressed() {
        let pool = mem_pool().await;
        let first = insert_at(&pool, "2026-07-19T10:00:00Z", 0.42).await;
        // 61s later — outside the window, so this is a new observation.
        let second = insert_at(&pool, "2026-07-19T10:01:01Z", 0.42).await;
        assert_ne!(second, first);
        assert_eq!(row_count(&pool).await, 2);
    }

    #[tokio::test]
    async fn a_moved_price_is_not_suppressed() {
        let pool = mem_pool().await;
        let first = insert_at(&pool, "2026-07-19T10:00:00Z", 0.42).await;
        let second = insert_at(&pool, "2026-07-19T10:00:10Z", 0.55).await;
        assert_ne!(second, first, "the market moved — that is real new information");
        assert_eq!(row_count(&pool).await, 2);
    }

    #[tokio::test]
    async fn a_different_ticker_is_never_suppressed() {
        let pool = mem_pool().await;
        insert_at(&pool, "2026-07-19T10:00:00Z", 0.42).await;
        let other = insert_forecast(
            &pool,
            "KXDUP-26JUL191215CWSTOR-TOR9",
            "2026-07-19T10:00:01Z",
            "2026-07-19T20:00:00Z",
            0.42,
            None,
            0.42,
            "pass",
            r#"[]"#,
            None,
            None,
            test_provenance(),
        )
        .await
        .unwrap();
        assert!(other > 0);
        assert_eq!(row_count(&pool).await, 2);
    }

    #[tokio::test]
    async fn duplicate_guard_reads_the_rfc3339_timestamps_the_app_actually_writes() {
        // `chrono::Utc::now().to_rfc3339()` emits nanosecond precision and a
        // `+00:00` offset. If SQLite's `julianday` could not read that, the
        // guard would silently never fire.
        let pool = mem_pool().await;
        let first = insert_at(&pool, "2026-07-19T10:00:00.123456789+00:00", 0.42).await;
        let second = insert_at(&pool, "2026-07-19T10:00:20.987654321+00:00", 0.42).await;
        assert_eq!(second, first);
        assert_eq!(row_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn transactional_insert_suppresses_duplicates_too() {
        let pool = mem_pool().await;
        let first = insert_at(&pool, "2026-07-19T10:00:00Z", 0.42).await;
        let mut txn = pool.begin().await.unwrap();
        let second = insert_forecast_tx(
            &mut txn,
            "KXDUP-26JUL191215CWSTOR-TOR8",
            "2026-07-19T10:00:15Z",
            "2026-07-19T20:00:00Z",
            0.42,
            None,
            0.42,
            "pass",
            r#"[]"#,
            None,
            None,
            test_provenance(),
        )
        .await
        .unwrap();
        txn.commit().await.unwrap();
        assert_eq!(second, first);
        assert_eq!(row_count(&pool).await, 1);
    }
}