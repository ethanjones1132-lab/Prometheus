#![allow(dead_code)]
//! ═══════════════════════════════════════════════════════════════
//! SQLite-backed Prediction Storage
//!
//! Replaces the old JSON-file-per-session storage with a single
//! SQLite database managed via sqlx. Provides CRUD operations
//! for predictions, outcomes, and bet history.
//!
//! Schema:
//!   predictions  — core prediction data extracted from AI responses
//!   bet_history  — bankroll bet results linked to predictions
//!
//! On first run, migrates existing JSON data into SQLite.
//! ═══════════════════════════════════════════════════════════════

use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite, Row, Transaction};
use std::path::PathBuf;

use super::tracker::{
    Prediction, PredictionOutcome, PredictionRecord,
};

/// Database path: ~/.openclaw/kalshi-monster/predictions.db
fn db_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".openclaw/kalshi-monster/predictions.db")
}

/// Ensure the parent directory exists.
fn ensure_db_dir() -> Result<(), String> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create db dir: {}", e))?;
    }
    Ok(())
}

/// Latest schema version tracked by the migration ledger. Bump this when adding
/// a new migration and implement the corresponding `migration_XX_*` function.
const BASELINE_VERSION: i64 = 3;

/// Open a connection pool and run migrations.
pub async fn init_db() -> Result<Pool<Sqlite>, String> {
    ensure_db_dir()?;
    let path = db_path();
    let path_str = path.display().to_string().replace('\\', "/");
    let url = format!("sqlite:///{}?mode=rwc", path_str);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("PRAGMA journal_mode=WAL")
                    .execute(&mut *conn)
                    .await?;
                sqlx::query("PRAGMA foreign_keys=ON")
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .map_err(|e| format!("Failed to connect to SQLite: {}", e))?;

    ensure_migrations_table(&pool).await?;
    baseline_existing_db(&pool).await?;
    run_migrations(&pool).await?;

    Ok(pool)
}

// ═══════════════════════════════════════════════════════════════
// Migration ledger
// ═══════════════════════════════════════════════════════════════

async fn ensure_migrations_table(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS _migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create _migrations table: {}", e))?;
    Ok(())
}

async fn current_migration_version(pool: &Pool<Sqlite>) -> Result<i64, String> {
    let version: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _migrations")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to read migration version: {}", e))?;
    Ok(version)
}

async fn predictions_table_exists(pool: &Pool<Sqlite>) -> Result<bool, String> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'predictions')",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to check predictions table: {}", e))?;
    Ok(exists)
}

/// Existing databases that predate the migration ledger are baselined by
/// running all current migrations idempotently and recording them as applied.
async fn baseline_existing_db(pool: &Pool<Sqlite>) -> Result<(), String> {
    if current_migration_version(pool).await? > 0 {
        return Ok(());
    }
    if !predictions_table_exists(pool).await? {
        return Ok(());
    }

    let mut txn = pool
        .begin()
        .await
        .map_err(|e| format!("begin baseline migration: {}", e))?;
    migration_01_initial_schema_tx(&mut txn).await?;
    migration_02_prediction_delta_columns_tx(&mut txn).await?;
    migration_03_prediction_edge_columns_tx(&mut txn).await?;
    for version in 1..=BASELINE_VERSION {
        record_migration_tx(&mut txn, version).await?;
    }
    txn.commit()
        .await
        .map_err(|e| format!("commit baseline migration: {}", e))?;
    Ok(())
}

/// Run every migration newer than the recorded version inside its own
/// transaction. The migration row is inserted before commit so a crash midway
/// leaves the ledger consistent with the schema that was actually applied.
async fn run_migrations(pool: &Pool<Sqlite>) -> Result<(), String> {
    let current = current_migration_version(pool).await?;
    for version in (current + 1)..=BASELINE_VERSION {
        let mut txn = pool
            .begin()
            .await
            .map_err(|e| format!("begin migration {version}: {}", e))?;
        match version {
            1 => migration_01_initial_schema_tx(&mut txn).await?,
            2 => migration_02_prediction_delta_columns_tx(&mut txn).await?,
            3 => migration_03_prediction_edge_columns_tx(&mut txn).await?,
            _ => return Err(format!("Unknown migration version: {}", version)),
        }
        record_migration_tx(&mut txn, version).await?;
        txn.commit()
            .await
            .map_err(|e| format!("commit migration {version}: {}", e))?;
    }
    Ok(())
}

async fn record_migration_tx(txn: &mut Transaction<'_, Sqlite>, version: i64) -> Result<(), String> {
    let name = migration_name(version);
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO _migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
    )
    .bind(version)
    .bind(name)
    .bind(&now)
    .execute(&mut **txn)
    .await
    .map_err(|e| format!("Failed to record migration {version}: {}", e))?;
    Ok(())
}

fn migration_name(version: i64) -> &'static str {
    match version {
        1 => "initial_schema",
        2 => "prediction_delta_columns",
        3 => "prediction_edge_columns",
        _ => "unknown",
    }
}

async fn existing_columns_tx(
    txn: &mut Transaction<'_, Sqlite>,
    table: &str,
) -> Result<std::collections::HashSet<String>, String> {
    let rows = sqlx::query(&format!("PRAGMA table_info({})", table))
        .fetch_all(&mut **txn)
        .await
        .map_err(|e| format!("PRAGMA table_info({}) failed: {}", table, e))?;
    Ok(rows.iter().map(|r| r.get::<String, _>("name")).collect())
}

async fn migration_01_initial_schema_tx(txn: &mut Transaction<'_, Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS predictions (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            raw_text TEXT NOT NULL DEFAULT '',
            player_name TEXT,
            pick_type TEXT,
            line REAL,
            stat_category TEXT,
            confidence TEXT,
            confidence_score INTEGER,
            probability REAL,
            reasoning TEXT,
            risk TEXT,
            created_at TEXT NOT NULL,
            outcome TEXT NOT NULL DEFAULT 'Pending',
            actual_result REAL,
            notes TEXT,
            resolved_at TEXT,
            entry_price REAL DEFAULT 0,
            close_price REAL DEFAULT 0,
            clv REAL DEFAULT 0,
            model_disagreement INTEGER DEFAULT 0
        )
        "#,
    )
    .execute(&mut **txn)
    .await
    .map_err(|e| format!("Migration 1 predictions table: {}", e))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS bet_history (
            id TEXT PRIMARY KEY,
            prediction_id TEXT,
            player_name TEXT NOT NULL,
            prop_category TEXT NOT NULL,
            line REAL NOT NULL,
            pick_type TEXT NOT NULL,
            stake REAL NOT NULL,
            odds REAL,
            outcome TEXT NOT NULL,
            profit_loss REAL NOT NULL DEFAULT 0.0,
            created_at TEXT NOT NULL,
            FOREIGN KEY (prediction_id) REFERENCES predictions(id)
        )
        "#,
    )
    .execute(&mut **txn)
    .await
    .map_err(|e| format!("Migration 1 bet_history table: {}", e))?;

    let indexes = [
        "CREATE INDEX IF NOT EXISTS idx_pred_session ON predictions(session_id)",
        "CREATE INDEX IF NOT EXISTS idx_pred_outcome ON predictions(outcome)",
        "CREATE INDEX IF NOT EXISTS idx_pred_player ON predictions(player_name)",
        "CREATE INDEX IF NOT EXISTS idx_pred_created ON predictions(created_at)",
        "CREATE INDEX IF NOT EXISTS idx_pred_outcome_created ON predictions(outcome, created_at)",
        "CREATE INDEX IF NOT EXISTS idx_pred_confidence ON predictions(confidence_score)",
    ];
    for sql in indexes {
        sqlx::query(sql)
            .execute(&mut **txn)
            .await
            .map_err(|e| format!("Migration 1 index ({sql}): {}", e))?;
    }

    Ok(())
}

async fn migration_02_prediction_delta_columns_tx(
    txn: &mut Transaction<'_, Sqlite>,
) -> Result<(), String> {
    let existing = existing_columns_tx(txn, "predictions").await?;
    let additions: &[(&str, &str)] = &[
        ("entry_price", "REAL DEFAULT 0"),
        ("close_price", "REAL DEFAULT 0"),
        ("clv", "REAL DEFAULT 0"),
        ("model_disagreement", "INTEGER DEFAULT 0"),
    ];
    for (name, ty) in additions {
        if existing.contains(*name) {
            continue;
        }
        let sql = format!("ALTER TABLE predictions ADD COLUMN {name} {ty}");
        sqlx::query(&sql)
            .execute(&mut **txn)
            .await
            .map_err(|e| format!("Migration 2 add column {name}: {}", e))?;
    }
    Ok(())
}

async fn migration_03_prediction_edge_columns_tx(
    txn: &mut Transaction<'_, Sqlite>,
) -> Result<(), String> {
    let existing = existing_columns_tx(txn, "predictions").await?;
    let additions: &[(&str, &str)] = &[
        ("full_decision_json", "TEXT"),
        ("p_market", "REAL"),
        ("p_model", "REAL"),
        ("p_final", "REAL"),
        ("verdict", "TEXT"),
        ("verdict_reasons", "TEXT"),
        ("agent_breakdown", "TEXT"),
        ("forecast_id", "INTEGER"),
    ];
    for (name, ty) in additions {
        if existing.contains(*name) {
            continue;
        }
        let sql = format!("ALTER TABLE predictions ADD COLUMN {name} {ty}");
        sqlx::query(&sql)
            .execute(&mut **txn)
            .await
            .map_err(|e| format!("Migration 3 add column {name}: {}", e))?;
    }
    Ok(())
}

/// Attach edge-engine outputs to an existing prediction row (paper/LLM path).
pub async fn update_prediction_edge_fields(
    pool: &Pool<Sqlite>,
    prediction_id: &str,
    p_market: f64,
    p_model: Option<f64>,
    p_final: f64,
    verdict: &str,
    verdict_reasons: &str,
    agent_breakdown: Option<&str>,
    forecast_id: Option<i64>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE predictions
        SET p_market = ?1,
            p_model = ?2,
            p_final = ?3,
            verdict = ?4,
            verdict_reasons = ?5,
            agent_breakdown = ?6,
            forecast_id = ?7
        WHERE id = ?8
        "#,
    )
    .bind(p_market)
    .bind(p_model)
    .bind(p_final)
    .bind(verdict)
    .bind(verdict_reasons)
    .bind(agent_breakdown)
    .bind(forecast_id)
    .bind(prediction_id)
    .execute(pool)
    .await
    .map_err(|e| format!("update prediction edge fields: {e}"))?;
    Ok(())
}

/// Transaction-aware version of `update_prediction_edge_fields`.
pub async fn update_prediction_edge_fields_tx(
    txn: &mut Transaction<'_, Sqlite>,
    prediction_id: &str,
    p_market: f64,
    p_model: Option<f64>,
    p_final: f64,
    verdict: &str,
    verdict_reasons: &str,
    agent_breakdown: Option<&str>,
    forecast_id: Option<i64>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE predictions
        SET p_market = ?1,
            p_model = ?2,
            p_final = ?3,
            verdict = ?4,
            verdict_reasons = ?5,
            agent_breakdown = ?6,
            forecast_id = ?7
        WHERE id = ?8
        "#,
    )
    .bind(p_market)
    .bind(p_model)
    .bind(p_final)
    .bind(verdict)
    .bind(verdict_reasons)
    .bind(agent_breakdown)
    .bind(forecast_id)
    .bind(prediction_id)
    .execute(&mut **txn)
    .await
    .map_err(|e| format!("update prediction edge fields: {e}"))?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// CRUD Operations
// ═══════════════════════════════════════════════════════════════

/// Insert a prediction record. Ignores duplicates (same id).
pub async fn insert_prediction(
    pool: &Pool<Sqlite>,
    record: &PredictionRecord,
) -> Result<(), String> {
    let p = &record.prediction;
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO predictions
            (id, session_id, raw_text, player_name, pick_type, line,
             stat_category, confidence, confidence_score, probability,
             reasoning, risk, created_at, outcome, actual_result, notes, resolved_at,
             full_decision_json, entry_price, model_disagreement)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
        "#,
    )
    .bind(&p.id)
    .bind(&p.session_id)
    .bind(&p.raw_text)
    .bind(&p.player_name)
    .bind(&p.pick_type)
    .bind(p.line)
    .bind(&p.stat_category)
    .bind(&p.confidence)
    .bind(p.confidence_score.map(|v| v as i64))
    .bind(p.probability)
    .bind(&p.reasoning)
    .bind(&p.risk)
    .bind(&p.created_at)
    .bind(record.outcome.to_string())
    .bind(record.actual_result)
    .bind(&record.notes)
    .bind(&record.resolved_at)
    .bind(&p.full_decision_json)
    .bind(p.entry_price)
    .bind(p.model_disagreement as i64)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to insert prediction: {}", e))?;

    Ok(())
}

/// Transaction-aware version of `insert_prediction`.
pub async fn insert_prediction_tx(
    txn: &mut Transaction<'_, Sqlite>,
    record: &PredictionRecord,
) -> Result<(), String> {
    let p = &record.prediction;
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO predictions
            (id, session_id, raw_text, player_name, pick_type, line,
             stat_category, confidence, confidence_score, probability,
             reasoning, risk, created_at, outcome, actual_result, notes, resolved_at,
             full_decision_json, entry_price, model_disagreement)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
        "#,
    )
    .bind(&p.id)
    .bind(&p.session_id)
    .bind(&p.raw_text)
    .bind(&p.player_name)
    .bind(&p.pick_type)
    .bind(p.line)
    .bind(&p.stat_category)
    .bind(&p.confidence)
    .bind(p.confidence_score.map(|v| v as i64))
    .bind(p.probability)
    .bind(&p.reasoning)
    .bind(&p.risk)
    .bind(&p.created_at)
    .bind(record.outcome.to_string())
    .bind(record.actual_result)
    .bind(&record.notes)
    .bind(&record.resolved_at)
    .bind(&p.full_decision_json)
    .bind(p.entry_price)
    .bind(p.model_disagreement as i64)
    .execute(&mut **txn)
    .await
    .map_err(|e| format!("Failed to insert prediction: {}", e))?;

    Ok(())
}

/// Update the outcome of a prediction.
pub async fn update_prediction_outcome(
    pool: &Pool<Sqlite>,
    prediction_id: &str,
    outcome: &PredictionOutcome,
    actual_result: Option<f64>,
) -> Result<(), String> {
    let resolved_at = if *outcome != PredictionOutcome::Pending {
        Some(chrono::Utc::now().to_rfc3339())
    } else {
        None
    };

    let rows = sqlx::query(
        r#"
        UPDATE predictions
        SET outcome = ?1, actual_result = ?2, resolved_at = ?3
        WHERE id = ?4
        "#,
    )
    .bind(outcome.to_string())
    .bind(actual_result)
    .bind(&resolved_at)
    .bind(prediction_id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to update prediction: {}", e))?
    .rows_affected();

    if rows == 0 {
        Err(format!("Prediction {} not found", prediction_id))
    } else {
        Ok(())
    }
}

/// Update CLV (Closing Line Value) for a prediction.
/// CLV = close_price - entry_price (in cents).
/// Positive CLV means the market moved in your favor after entry.
pub async fn update_prediction_clv(
    pool: &Pool<Sqlite>,
    prediction_id: &str,
    close_price: f64,
) -> Result<(), String> {
    // First get the entry_price
    let row = sqlx::query("SELECT entry_price FROM predictions WHERE id = ?1")
        .bind(prediction_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Failed to fetch entry_price: {}", e))?;

    let entry_price: f64 = match row {
        Some(r) => r.get::<f64, _>("entry_price"),
        None => return Err(format!("Prediction {} not found", prediction_id)),
    };

    let clv = close_price - entry_price;

    let rows = sqlx::query(
        r#"
        UPDATE predictions
        SET close_price = ?1, clv = ?2
        WHERE id = ?3
        "#,
    )
    .bind(close_price)
    .bind(clv)
    .bind(prediction_id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to update CLV: {}", e))?
    .rows_affected();

    if rows == 0 {
        Err(format!("Prediction {} not found", prediction_id))
    } else {
        Ok(())
    }
}

/// Set the entry price for a prediction (called when a trade decision is recorded).
pub async fn set_prediction_entry_price(
    pool: &Pool<Sqlite>,
    prediction_id: &str,
    entry_price: f64,
) -> Result<(), String> {
    let rows = sqlx::query(
        r#"
        UPDATE predictions
        SET entry_price = ?1
        WHERE id = ?2
        "#,
    )
    .bind(entry_price)
    .bind(prediction_id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to set entry_price: {}", e))?
    .rows_affected();

    if rows == 0 {
        Err(format!("Prediction {} not found", prediction_id))
    } else {
        Ok(())
    }
}

/// Set model disagreement flag for a prediction.
pub async fn set_model_disagreement(
    pool: &Pool<Sqlite>,
    prediction_id: &str,
    disagreement: bool,
) -> Result<(), String> {
    let rows = sqlx::query(
        r#"
        UPDATE predictions
        SET model_disagreement = ?1
        WHERE id = ?2
        "#,
    )
    .bind(if disagreement { 1 } else { 0 })
    .bind(prediction_id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to set model_disagreement: {}", e))?
    .rows_affected();

    if rows == 0 {
        Err(format!("Prediction {} not found", prediction_id))
    } else {
        Ok(())
    }
}

/// Get all predictions for a session, ordered by created_at desc.
pub async fn get_session_predictions(
    pool: &Pool<Sqlite>,
    session_id: &str,
) -> Result<Vec<PredictionRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, session_id, raw_text, player_name, pick_type, line,
               stat_category, confidence, confidence_score, probability,
               reasoning, risk, created_at, outcome, actual_result, notes, resolved_at,
               full_decision_json
        FROM predictions
        WHERE session_id = ?1
        ORDER BY created_at DESC
        "#,
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch session predictions: {}", e))?;

    Ok(rows.iter().map(row_to_record).collect())
}

/// Get all predictions across all sessions, ordered by created_at desc.
pub async fn get_all_predictions(pool: &Pool<Sqlite>) -> Result<Vec<PredictionRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, session_id, raw_text, player_name, pick_type, line,
               stat_category, confidence, confidence_score, probability,
               reasoning, risk, created_at, outcome, actual_result, notes, resolved_at,
               full_decision_json, entry_price, close_price, clv, model_disagreement
        FROM predictions
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch all predictions: {}", e))?;

    Ok(rows.iter().map(row_to_record).collect())
}

/// Get predictions filtered by confidence score range.
pub async fn get_predictions_by_confidence(
    pool: &Pool<Sqlite>,
    min_score: u8,
    max_score: u8,
) -> Result<Vec<PredictionRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, session_id, raw_text, player_name, pick_type, line,
               stat_category, confidence, confidence_score, probability,
               reasoning, risk, created_at, outcome, actual_result, notes, resolved_at,
               full_decision_json, entry_price, model_disagreement
        FROM predictions
        WHERE confidence_score >= ?1 AND confidence_score <= ?2
        ORDER BY created_at DESC
        "#,
    )
    .bind(min_score as i64)
    .bind(max_score as i64)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch predictions by confidence: {}", e))?;

    Ok(rows.iter().map(row_to_record).collect())
}

/// Delete a prediction by id.
pub async fn delete_prediction(pool: &Pool<Sqlite>, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM predictions WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to delete prediction: {}", e))?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// Bet History CRUD
// ═══════════════════════════════════════════════════════════════

/// A recorded bet result.
#[derive(Debug, Clone)]
pub struct BetRecord {
    pub id: String,
    pub prediction_id: Option<String>,
    pub player_name: String,
    pub prop_category: String,
    pub line: f64,
    pub pick_type: String,
    pub stake: f64,
    pub odds: Option<f64>,
    pub outcome: String,
    pub profit_loss: f64,
    pub created_at: String,
}

/// Insert a bet history record.
pub async fn insert_bet(pool: &Pool<Sqlite>, record: &BetRecord) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO bet_history
            (id, prediction_id, player_name, prop_category, line, pick_type,
             stake, odds, outcome, profit_loss, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        "#,
    )
    .bind(&record.id)
    .bind(&record.prediction_id)
    .bind(&record.player_name)
    .bind(&record.prop_category)
    .bind(record.line)
    .bind(&record.pick_type)
    .bind(record.stake)
    .bind(record.odds)
    .bind(&record.outcome)
    .bind(record.profit_loss)
    .bind(&record.created_at)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to insert bet history: {}", e))?;

    Ok(())
}

/// Get all bet history records, ordered by created_at desc.
pub async fn get_bet_history(pool: &Pool<Sqlite>) -> Result<Vec<BetRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, prediction_id, player_name, prop_category, line, pick_type,
               stake, odds, outcome, profit_loss, created_at
        FROM bet_history
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch bet history: {}", e))?;

    Ok(rows
        .iter()
        .map(|r| BetRecord {
            id: r.get("id"),
            prediction_id: r.get("prediction_id"),
            player_name: r.get("player_name"),
            prop_category: r.get("prop_category"),
            line: r.get("line"),
            pick_type: r.get("pick_type"),
            stake: r.get("stake"),
            odds: r.get("odds"),
            outcome: r.get("outcome"),
            profit_loss: r.get("profit_loss"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// Get total profit/loss from bet history.
pub async fn get_total_profit_loss(pool: &Pool<Sqlite>) -> Result<f64, String> {
    let row = sqlx::query("SELECT COALESCE(SUM(profit_loss), 0.0) as total FROM bet_history")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Failed to fetch total P&L: {}", e))?;

    Ok(row.get::<f64, _>("total"))
}

// ═══════════════════════════════════════════════════════════════
// JSON → SQLite Migration
// ═══════════════════════════════════════════════════════════════

/// Migrate existing JSON prediction files into SQLite.
/// Called once on startup. Safe to call multiple times (INSERT OR IGNORE).
pub async fn migrate_from_json(pool: &Pool<Sqlite>) -> Result<usize, String> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let predictions_dir = PathBuf::from(home)
        .join(".openclaw/kalshi-monster/predictions");

    if !predictions_dir.exists() {
        return Ok(0);
    }

    let mut migrated = 0usize;

    let entries = std::fs::read_dir(&predictions_dir)
        .map_err(|e| format!("Failed to read predictions dir: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let records: Vec<PredictionRecord> = match serde_json::from_str(&content) {
            Ok(r) => r,
            Err(_) => continue,
        };

        for record in &records {
            if let Err(e) = insert_prediction(pool, record).await {
                tracing::warn!("Failed to migrate prediction {}: {}", record.prediction.id, e);
            } else {
                migrated += 1;
            }
        }
    }

    if migrated > 0 {
        tracing::info!("Migrated {} predictions from JSON to SQLite", migrated);
    }

    Ok(migrated)
}

// ═══════════════════════════════════════════════════════════════
// Row → PredictionRecord conversion
// ═══════════════════════════════════════════════════════════════

fn row_to_record(r: &sqlx::sqlite::SqliteRow) -> PredictionRecord {
    let outcome_str: String = r.get("outcome");
    let outcome = outcome_str.parse::<PredictionOutcome>().unwrap_or(PredictionOutcome::Pending);

    let confidence_score: Option<i64> = r.get("confidence_score");

    PredictionRecord {
        prediction: Prediction {
            id: r.get("id"),
            session_id: r.get("session_id"),
            raw_text: r.get("raw_text"),
            player_name: r.get("player_name"),
            pick_type: r.get("pick_type"),
            line: r.get("line"),
            stat_category: r.get("stat_category"),
            confidence: r.get("confidence"),
            confidence_score: confidence_score.map(|v| v as u8),
            probability: r.get("probability"),
            reasoning: r.get("reasoning"),
            risk: r.get("risk"),
            created_at: r.get("created_at"),
            full_decision_json: r.try_get("full_decision_json").ok().flatten(),
            entry_price: r.try_get("entry_price").ok().or_else(|| {
                r.try_get::<f64, _>("entry_price").ok().map(Some).flatten()
            }),
            close_price: r.try_get("close_price").ok().or_else(|| {
                r.try_get::<f64, _>("close_price")
                    .ok()
                    .filter(|v| *v != 0.0)
                    .map(Some)
                    .flatten()
            }),
            clv: r.try_get("clv").ok().or_else(|| {
                r.try_get::<f64, _>("clv")
                    .ok()
                    .filter(|v| *v != 0.0)
                    .map(Some)
                    .flatten()
            }),
            model_disagreement: r.try_get::<i64, _>("model_disagreement").ok().unwrap_or(0) != 0,
        },
        outcome,
        actual_result: r.get("actual_result"),
        notes: r.get("notes"),
        resolved_at: r.get("resolved_at"),
    }
}
