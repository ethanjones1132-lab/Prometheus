//! Real Kalshi paper-trading engine.
//!
//! Tracks an immutable lot journal, a cash account, and equity snapshots
//! independently of the real-money Kalshi grading path. Payout math follows
//! Kalshi's binary contract rules: each contract pays $1 if the held side
//! wins and $0 if it loses.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};

use crate::edge_engine::{order_fee, EdgeConfig};
use crate::kalshi::client::KalshiClient;
use crate::predictions::tracker::PredictionRecord;


pub const PAPER_SESSION_ID: &str = "paper-sim";
const DEFAULT_STARTING_BALANCE: f64 = 10_000.0;

async fn edge_fee_multiplier(pool: &Pool<Sqlite>) -> f64 {
    crate::edge_engine::persistence::load_edge_config(pool)
        .await
        .map(|c| c.fee_multiplier)
        .unwrap_or_else(|_| EdgeConfig::default().fee_multiplier)
}

/// Singleton paper account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperAccount {
    pub id: i64,
    pub balance_dollars: f64,
    pub total_deposits: f64,
    pub total_withdrawals: f64,
    pub created_at: String,
    pub updated_at: String,
}

/// How a paper trade was created.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PaperTradeSource {
    AiDecision,
    Manual,
}

impl PaperTradeSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaperTradeSource::AiDecision => "AiDecision",
            PaperTradeSource::Manual => "Manual",
        }
    }
}

impl std::str::FromStr for PaperTradeSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "AiDecision" => Ok(PaperTradeSource::AiDecision),
            "Manual" => Ok(PaperTradeSource::Manual),
            _ => Err(format!("unknown paper trade source: {}", s)),
        }
    }
}

/// An immutable fill (lot). Closed lots record realized PnL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperLot {
    pub id: String,
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub side: String,
    pub entry_price_cents: f64,
    pub qty: f64,
    /// Total cash spent to open the lot: gross cost + Kalshi taker fee.
    pub stake_dollars: f64,
    pub source: PaperTradeSource,
    pub decision_json: Option<String>,
    /// Linked predictions.id when opened via kalshi_record_paper_decision.
    pub prediction_id: Option<String>,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub closed_price_cents: Option<f64>,
    pub realized_pnl: Option<f64>,
    pub status: String,
    pub settlement_result: Option<String>,
}

/// Input used to open a new paper position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperTradeInput {
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub side: String,
    pub qty: f64,
    pub entry_price_cents: f64,
    pub source: PaperTradeSource,
    pub decision_json: Option<String>,
    /// Optional link back to the predictions journal row.
    #[serde(default)]
    pub prediction_id: Option<String>,
}

/// An aggregated open position per ticker/side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperPosition {
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub side: String,
    pub total_qty: f64,
    pub avg_entry_price_cents: f64,
    pub cost_basis_dollars: f64,
    pub mark_price_cents: Option<f64>,
    pub market_value_dollars: Option<f64>,
    pub unrealized_pnl_dollars: Option<f64>,
    pub lots_count: i64,
}

/// Structured result of `kalshi_record_paper_decision` (Sprint 0.1).
/// Lets the UI show whether a cash lot opened vs journal-only, and any demotions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperRecordResult {
    pub prediction_id: String,
    /// True only when a `paper_lots` row was opened and cash was debited.
    pub lot_opened: bool,
    /// Optional lot id when `lot_opened`.
    pub lot_id: Option<String>,
    /// Final decision after rails/gates (TAKE / WATCH / PASS).
    pub final_decision: String,
    pub contract_side: String,
    pub ticker: String,
    /// Final recommended stake after caps/breakers (0 if no lot).
    pub stake: f64,
    pub price_to_enter: f64,
    /// Human-readable demotion / rail notes for the UI.
    pub demotion_notes: Vec<String>,
    /// True when breakers blocked opening a new lot (daily pause / hard disable).
    pub paper_lots_blocked: bool,
}

/// Result of a settlement run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperSettlementSummary {
    pub settled: u32,
    pub wins: u32,
    pub losses: u32,
    pub total_pnl: f64,
    pub details: Vec<PaperSettlementDetail>,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperSettlementDetail {
    pub lot_id: String,
    pub ticker: String,
    pub side: String,
    pub result: String,
    pub realized_pnl: f64,
    /// Predictions journal row graded as a side-effect of settlement (if any).
    #[serde(default)]
    pub prediction_id: Option<String>,
    /// Win / Loss / Push written to the prediction (if synced).
    #[serde(default)]
    pub prediction_outcome: Option<String>,
}

/// High-level paper-trading analytics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperAnalytics {
    pub starting_balance: f64,
    pub cash_balance: f64,
    pub open_market_value: f64,
    pub equity: f64,
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,
    pub total_return_pct: f64,
    pub total_trades: u32,
    pub open_positions: u32,
    pub win_rate: f64,
    pub wins: u32,
    pub losses: u32,
    pub profit_factor: f64,
    pub avg_winner: f64,
    pub avg_loser: f64,
    pub largest_winner: f64,
    pub largest_loser: f64,
    pub max_drawdown_pct: f64,
    pub fetched_at: String,
}

/// Equity snapshot used for drawdown and trend charts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperEquitySnapshot {
    pub id: i64,
    pub ts: String,
    pub balance_dollars: f64,
    pub open_market_value: f64,
    pub equity_dollars: f64,
    pub unrealized_pnl: f64,
}

// ═══════════════════════════════════════════════════════════════
// Schema & bootstrap
// ═══════════════════════════════════════════════════════════════

pub async fn init_paper_tables(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS paper_account (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            balance_dollars REAL NOT NULL,
            total_deposits REAL NOT NULL DEFAULT 0.0,
            total_withdrawals REAL NOT NULL DEFAULT 0.0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create paper_account table: {}", e))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS paper_lots (
            id TEXT PRIMARY KEY,
            ticker TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            category TEXT NOT NULL DEFAULT 'Other',
            side TEXT NOT NULL,
            entry_price_cents REAL NOT NULL,
            qty REAL NOT NULL,
            stake_dollars REAL NOT NULL,
            source TEXT NOT NULL DEFAULT 'Manual',
            decision_json TEXT,
            opened_at TEXT NOT NULL,
            closed_at TEXT,
            closed_price_cents REAL,
            realized_pnl REAL,
            status TEXT NOT NULL DEFAULT 'Open',
            settlement_result TEXT
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create paper_lots table: {}", e))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS paper_equity_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            balance_dollars REAL NOT NULL,
            open_market_value REAL NOT NULL,
            equity_dollars REAL NOT NULL,
            unrealized_pnl REAL NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create paper_equity_snapshots table: {}", e))?;

    // Indexes
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_paper_lots_ticker ON paper_lots(ticker)")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_paper_lots_status ON paper_lots(status)")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_paper_lots_opened ON paper_lots(opened_at)")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_paper_equity_ts ON paper_equity_snapshots(ts)")
        .execute(pool)
        .await
        .ok();

    // Migration: link lots → predictions for grade sync on settle.
    let _ = sqlx::query(
        "ALTER TABLE paper_lots ADD COLUMN prediction_id TEXT",
    )
    .execute(pool)
    .await;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_paper_lots_prediction ON paper_lots(prediction_id)",
    )
    .execute(pool)
    .await
    .ok();

    // Bootstrap singleton account if missing.
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM paper_account WHERE id = 1)")
        .fetch_one(pool)
        .await
        .unwrap_or(false);
    if !exists {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO paper_account (id, balance_dollars, total_deposits, total_withdrawals, created_at, updated_at) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
        )
        .bind(DEFAULT_STARTING_BALANCE)
        .bind(DEFAULT_STARTING_BALANCE)
        .bind(0.0)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to bootstrap paper account: {}", e))?;
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// Account
// ═══════════════════════════════════════════════════════════════

pub async fn get_account(pool: &Pool<Sqlite>) -> Result<PaperAccount, String> {
    let row = sqlx::query(
        "SELECT id, balance_dollars, total_deposits, total_withdrawals, created_at, updated_at FROM paper_account WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper account: {}", e))?;

    Ok(PaperAccount {
        id: row.get("id"),
        balance_dollars: row.get("balance_dollars"),
        total_deposits: row.get("total_deposits"),
        total_withdrawals: row.get("total_withdrawals"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

async fn get_account_tx(txn: &mut sqlx::Transaction<'_, Sqlite>) -> Result<PaperAccount, String> {
    let row = sqlx::query(
        "SELECT id, balance_dollars, total_deposits, total_withdrawals, created_at, updated_at FROM paper_account WHERE id = 1",
    )
    .fetch_one(&mut **txn)
    .await
    .map_err(|e| format!("Failed to fetch paper account: {}", e))?;

    Ok(PaperAccount {
        id: row.get("id"),
        balance_dollars: row.get("balance_dollars"),
        total_deposits: row.get("total_deposits"),
        total_withdrawals: row.get("total_withdrawals"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub async fn reset_account(
    pool: &Pool<Sqlite>,
    starting_balance: Option<f64>,
) -> Result<PaperAccount, String> {
    let balance = starting_balance.unwrap_or(DEFAULT_STARTING_BALANCE).max(0.0);
    let now = Utc::now().to_rfc3339();

    sqlx::query("DELETE FROM paper_lots")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to clear paper lots: {}", e))?;
    sqlx::query("DELETE FROM paper_equity_snapshots")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to clear paper snapshots: {}", e))?;

    sqlx::query(
        "INSERT OR REPLACE INTO paper_account (id, balance_dollars, total_deposits, total_withdrawals, created_at, updated_at) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
    )
    .bind(balance)
    .bind(balance)
    .bind(0.0)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to reset paper account: {}", e))?;

    Ok(get_account(pool).await?)
}

async fn update_balance(pool: &Pool<Sqlite>, delta: f64) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE paper_account SET balance_dollars = balance_dollars + ?1, updated_at = ?2 WHERE id = 1",
    )
    .bind(delta)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to update paper balance: {}", e))?;
    Ok(())
}

async fn update_balance_tx(txn: &mut sqlx::Transaction<'_, Sqlite>, delta: f64) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE paper_account SET balance_dollars = balance_dollars + ?1, updated_at = ?2 WHERE id = 1",
    )
    .bind(delta)
    .bind(&now)
    .execute(&mut **txn)
    .await
    .map_err(|e| format!("Failed to update paper balance: {}", e))?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// Lots & trades
// ═══════════════════════════════════════════════════════════════

fn normalize_side(side: &str) -> Result<String, String> {
    let upper = side.trim().to_ascii_uppercase();
    if upper == "YES" || upper == "NO" {
        Ok(upper)
    } else {
        Err(format!("Invalid paper trade side: {}", side))
    }
}

pub async fn place_trade(pool: &Pool<Sqlite>, input: PaperTradeInput) -> Result<PaperLot, String> {
    let fee_multiplier = edge_fee_multiplier(pool).await;
    let mut txn = pool.begin().await.map_err(|e| format!("begin transaction: {e}"))?;
    let lot = place_trade_tx(&mut txn, &input, fee_multiplier).await?;
    txn.commit().await.map_err(|e| format!("commit paper trade: {e}"))?;

    // Snapshot is recorded after commit so mark-to-market reads do not hold
    // the transaction open.
    record_equity_snapshot(pool, None).await?;

    Ok(lot)
}

/// Transaction-aware core of `place_trade`. The caller is responsible for
/// committing `txn` and recording an equity snapshot afterwards.
pub async fn place_trade_tx(
    txn: &mut sqlx::Transaction<'_, Sqlite>,
    input: &PaperTradeInput,
    fee_multiplier: f64,
) -> Result<PaperLot, String> {
    let side = normalize_side(&input.side)?;
    if input.qty <= 0.0 {
        return Err("Paper trade quantity must be positive".into());
    }
    if input.entry_price_cents <= 0.0 || input.entry_price_cents >= 100.0 {
        return Err("Paper entry price must be between 0 and 100 cents".into());
    }

    let entry_price_dollars = input.entry_price_cents / 100.0;
    let cost = input.qty * entry_price_dollars;
    let fee = order_fee(entry_price_dollars, input.qty, fee_multiplier);
    let total_cost = cost + fee;
    let account = get_account_tx(txn).await?;
    if total_cost > account.balance_dollars {
        return Err(format!(
            "Insufficient paper buying power: ${:.2} needed, ${:.2} available",
            total_cost, account.balance_dollars
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let source_str = input.source.as_str().to_string();

    sqlx::query(
        r#"
        INSERT INTO paper_lots
            (id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
             source, decision_json, prediction_id, opened_at, status)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'Open')
        "#,
    )
    .bind(&id)
    .bind(&input.ticker)
    .bind(&input.title)
    .bind(&input.category)
    .bind(&side)
    .bind(input.entry_price_cents)
    .bind(input.qty)
    .bind(total_cost)
    .bind(&source_str)
    .bind(&input.decision_json)
    .bind(&input.prediction_id)
    .bind(&now)
    .execute(&mut **txn)
    .await
    .map_err(|e| format!("Failed to insert paper lot: {}", e))?;

    update_balance_tx(txn, -total_cost).await?;

    get_lot_tx(txn, &id).await
}

/// Bundles all database writes performed when recording a paper decision so
/// they can commit atomically.
#[derive(Clone)]
pub struct PaperDecisionContext {
    pub prediction: PredictionRecord,
    pub forecast_ticker: String,
    pub forecast_created_at: String,
    pub forecast_close_time: String,
    pub p_market: f64,
    pub p_model: Option<f64>,
    pub p_final: f64,
    pub verdict: String,
    pub verdict_reasons: String,
    pub stake_suggested: Option<f64>,
    pub agent_breakdown: Option<String>,
    /// Which code path produced this forecast ("app", "chat", a script name).
    pub forecast_source: String,
    /// How many agents actually returned a probability.
    pub forecast_agents_opining: Option<i64>,
    pub trade_input: Option<PaperTradeInput>,
}

/// Atomically record a paper decision: prediction, forecast, edge-field update,
/// and (if TAKE) paper lot + balance change all commit together.
/// Returns prediction id and optional opened lot id.
pub async fn record_paper_decision(
    pool: &Pool<Sqlite>,
    ctx: PaperDecisionContext,
) -> Result<(String, Option<String>), String> {
    let prediction_id = ctx.prediction.prediction.id.clone();
    let breakdown_slice = ctx.agent_breakdown.as_deref();
    let fee_multiplier = edge_fee_multiplier(pool).await;

    let mut txn = pool
        .begin()
        .await
        .map_err(|e| format!("begin paper decision transaction: {e}"))?;

    // 1. Prediction row.
    crate::predictions::storage::insert_prediction_tx(&mut txn, &ctx.prediction)
        .await
        .map_err(|e| format!("prediction insert: {e}"))?;

    // 2. Forecast ledger row.
    let forecast_id = crate::kalshi::forecast::insert_forecast_tx(
        &mut txn,
        &ctx.forecast_ticker,
        &ctx.forecast_created_at,
        &ctx.forecast_close_time,
        ctx.p_market,
        ctx.p_model,
        ctx.p_final,
        &ctx.verdict,
        &ctx.verdict_reasons,
        ctx.stake_suggested,
        breakdown_slice,
        crate::kalshi::forecast::ForecastProvenance {
            source: &ctx.forecast_source,
            agents_opining: ctx.forecast_agents_opining,
        },
    )
    .await
    .map_err(|e| format!("forecast insert: {e}"))?;

    // 3. Attach edge fields + forecast id to the prediction.
    crate::predictions::storage::update_prediction_edge_fields_tx(
        &mut txn,
        &prediction_id,
        ctx.p_market,
        ctx.p_model,
        ctx.p_final,
        &ctx.verdict,
        &ctx.verdict_reasons,
        breakdown_slice,
        Some(forecast_id),
    )
    .await
    .map_err(|e| format!("prediction edge update: {e}"))?;

    // 4. Optional paper lot (PASS/WATCH do not open a position).
    let mut opened_lot_id: Option<String> = None;
    if let Some(mut input) = ctx.trade_input {
        // Always link the lot to this prediction for settle → grade sync.
        if input.prediction_id.is_none() {
            input.prediction_id = Some(prediction_id.clone());
        }
        let lot = place_trade_tx(&mut txn, &input, fee_multiplier)
            .await
            .map_err(|e| format!("paper lot: {e}"))?;
        opened_lot_id = Some(lot.id);
    }

    txn.commit()
        .await
        .map_err(|e| format!("commit paper decision: {e}"))?;

    // Equity snapshot after commit — cost-basis fallback when no live marks.
    record_equity_snapshot(pool, None).await.ok();

    Ok((prediction_id, opened_lot_id))
}

/// Exit price in cents for the *held side* given Kalshi YES/NO settlement.
/// Held-side contracts pay $1 (100¢) when that side wins, else $0.
pub fn settlement_exit_cents_for_side(side: &str, actual_yes_no: &str) -> f64 {
    let side = side.trim().to_uppercase();
    let actual = actual_yes_no.trim();
    let held_wins = (side == "YES" && actual.eq_ignore_ascii_case("yes"))
        || (side == "NO" && actual.eq_ignore_ascii_case("no"));
    if held_wins {
        100.0
    } else {
        0.0
    }
}

pub async fn close_lot(
    pool: &Pool<Sqlite>,
    lot_id: &str,
    exit_price_cents: f64,
) -> Result<PaperLot, String> {
    close_lot_with_result(pool, lot_id, exit_price_cents, "Closed").await
}

/// Close a lot and store the Kalshi settlement string (Yes/No) when known.
pub async fn close_lot_with_result(
    pool: &Pool<Sqlite>,
    lot_id: &str,
    exit_price_cents: f64,
    settlement_result: &str,
) -> Result<PaperLot, String> {
    if exit_price_cents < 0.0 || exit_price_cents > 100.0 {
        return Err("Exit price must be between 0 and 100 cents".into());
    }

    let lot = get_lot(pool, lot_id).await?;
    if lot.status != "Open" {
        return Err(format!("Lot {} is not open", lot_id));
    }

    let proceeds = lot.qty * exit_price_cents / 100.0;
    let realized = proceeds - lot.stake_dollars;
    let now = Utc::now().to_rfc3339();
    let result = if !settlement_result.is_empty() && settlement_result != "Closed" {
        settlement_result.to_string()
    } else if exit_price_cents >= 99.99 {
        "Yes".to_string()
    } else if exit_price_cents <= 0.01 {
        "No".to_string()
    } else {
        "Closed".to_string()
    };

    sqlx::query(
        r#"
        UPDATE paper_lots
        SET closed_at = ?1, closed_price_cents = ?2, realized_pnl = ?3,
            status = 'Closed', settlement_result = ?4
        WHERE id = ?5
        "#,
    )
    .bind(&now)
    .bind(exit_price_cents)
    .bind(realized)
    .bind(&result)
    .bind(lot_id)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to close paper lot: {}", e))?;

    update_balance(pool, proceeds).await?;
    record_equity_snapshot(pool, None).await?;

    get_lot(pool, lot_id).await
}

pub async fn get_lot(pool: &Pool<Sqlite>, lot_id: &str) -> Result<PaperLot, String> {
    let row = sqlx::query(
        r#"
        SELECT id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
               source, decision_json, prediction_id, opened_at, closed_at, closed_price_cents,
               realized_pnl, status, settlement_result
        FROM paper_lots WHERE id = ?1
        "#,
    )
    .bind(lot_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper lot: {}", e))?;

    Ok(row_to_lot(&row))
}

async fn get_lot_tx(txn: &mut sqlx::Transaction<'_, Sqlite>, lot_id: &str) -> Result<PaperLot, String> {
    let row = sqlx::query(
        r#"
        SELECT id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
               source, decision_json, prediction_id, opened_at, closed_at, closed_price_cents,
               realized_pnl, status, settlement_result
        FROM paper_lots WHERE id = ?1
        "#,
    )
    .bind(lot_id)
    .fetch_one(&mut **txn)
    .await
    .map_err(|e| format!("Failed to fetch paper lot: {}", e))?;

    Ok(row_to_lot(&row))
}

pub async fn get_all_lots(pool: &Pool<Sqlite>) -> Result<Vec<PaperLot>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
               source, decision_json, prediction_id, opened_at, closed_at, closed_price_cents,
               realized_pnl, status, settlement_result
        FROM paper_lots ORDER BY opened_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch paper lots: {}", e))?;

    Ok(rows.iter().map(row_to_lot).collect())
}

pub async fn get_open_lots(pool: &Pool<Sqlite>) -> Result<Vec<PaperLot>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
               source, decision_json, prediction_id, opened_at, closed_at, closed_price_cents,
               realized_pnl, status, settlement_result
        FROM paper_lots WHERE status = 'Open' ORDER BY opened_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch open paper lots: {}", e))?;

    Ok(rows.iter().map(row_to_lot).collect())
}

fn row_to_lot(r: &sqlx::sqlite::SqliteRow) -> PaperLot {
    let source: String = r.get("source");
    PaperLot {
        id: r.get("id"),
        ticker: r.get("ticker"),
        title: r.get("title"),
        category: r.get("category"),
        side: r.get("side"),
        entry_price_cents: r.get("entry_price_cents"),
        qty: r.get("qty"),
        stake_dollars: r.get("stake_dollars"),
        source: source.parse().unwrap_or(PaperTradeSource::Manual),
        decision_json: r.get("decision_json"),
        prediction_id: r.try_get("prediction_id").ok().flatten(),
        opened_at: r.get("opened_at"),
        closed_at: r.get("closed_at"),
        closed_price_cents: r.get("closed_price_cents"),
        realized_pnl: r.get("realized_pnl"),
        status: r.get("status"),
        settlement_result: r.get("settlement_result"),
    }
}

// ═══════════════════════════════════════════════════════════════
// Aggregate positions
// ═══════════════════════════════════════════════════════════════

pub async fn aggregate_positions(
    pool: &Pool<Sqlite>,
    client: Option<&KalshiClient>,
) -> Result<Vec<PaperPosition>, String> {
    let open = get_open_lots(pool).await?;
    if open.is_empty() {
        return Ok(Vec::new());
    }

    // Group by ticker/side.
    let mut groups: std::collections::HashMap<(String, String), Vec<PaperLot>> =
        std::collections::HashMap::new();
    for lot in open {
        groups
            .entry((lot.ticker.clone(), lot.side.clone()))
            .or_default()
            .push(lot);
    }

    let mut positions = Vec::new();
    for ((ticker, side), lots) in groups {
        let total_qty: f64 = lots.iter().map(|l| l.qty).sum();
        let cost_basis: f64 = lots.iter().map(|l| l.stake_dollars).sum();
        let avg_entry = if total_qty > 0.0 {
            lots.iter().map(|l| l.entry_price_cents * l.qty).sum::<f64>() / total_qty
        } else {
            0.0
        };

        let (title, category) = lots
            .first()
            .map(|l| (l.title.clone(), l.category.clone()))
            .unwrap_or_default();

        let mark = if let Some(c) = client {
            best_bid_cents(c, &ticker, &side).await.ok()
        } else {
            None
        };

        let (market_value, unrealized) = mark.map(|m| {
            let mv = total_qty * m / 100.0;
            let ur = mv - cost_basis;
            (Some(mv), Some(ur))
        }).unwrap_or((None, None));

        positions.push(PaperPosition {
            ticker,
            title,
            category,
            side,
            total_qty,
            avg_entry_price_cents: avg_entry,
            cost_basis_dollars: cost_basis,
            mark_price_cents: mark,
            market_value_dollars: market_value,
            unrealized_pnl_dollars: unrealized,
            lots_count: lots.len() as i64,
        });
    }

    Ok(positions)
}

// ═══════════════════════════════════════════════════════════════
// Settlement against resolved Kalshi markets
// ═══════════════════════════════════════════════════════════════

pub async fn settle_pending(
    pool: &Pool<Sqlite>,
    client: &KalshiClient,
) -> Result<PaperSettlementSummary, String> {
    let open = get_open_lots(pool).await?;
    if open.is_empty() {
        return Ok(PaperSettlementSummary {
            settled: 0,
            wins: 0,
            losses: 0,
            total_pnl: 0.0,
            details: Vec::new(),
            fetched_at: Utc::now().to_rfc3339(),
        });
    }

    let mut by_ticker: std::collections::HashMap<String, Vec<PaperLot>> =
        std::collections::HashMap::new();
    for lot in open {
        by_ticker.entry(lot.ticker.clone()).or_default().push(lot);
    }

    let mut summary = PaperSettlementSummary {
        settled: 0,
        wins: 0,
        losses: 0,
        total_pnl: 0.0,
        details: Vec::new(),
        fetched_at: Utc::now().to_rfc3339(),
    };

    for (ticker, lots) in by_ticker {
        let market = match client.fetch_market(&ticker).await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("paper settle: skip {} — {}", ticker, e);
                continue;
            }
        };
        if market.result.is_empty() {
            continue;
        }

        // Normalize Yes/No (Kalshi may return mixed case).
        let actual = crate::kalshi::grading::normalize_settlement_result(&market.result);
        if actual != "Yes" && actual != "No" {
            tracing::debug!(
                "paper settle: skip {ticker} — non-binary result {:?}",
                market.result
            );
            continue;
        }

        // Resolve forecast ledger rows for this market (same truth as lot settle).
        let resolved_at = Utc::now().to_rfc3339();
        if let Err(e) = crate::kalshi::forecast::resolve_forecasts_for_market(
            pool,
            &ticker,
            &actual,
            &resolved_at,
        )
        .await
        {
            tracing::warn!("paper settle forecast resolve {ticker}: {e}");
        }

        for lot in lots {
            // CRITICAL: exit price is for the *held side*, not always the YES settlement.
            // YES lot + Yes → 100c; YES lot + No → 0c; NO lot + No → 100c; NO lot + Yes → 0c.
            let exit = settlement_exit_cents_for_side(&lot.side, &actual);
            let closed = close_lot_with_result(pool, &lot.id, exit, &actual).await?;
            let won = (closed.side == "YES" && actual == "Yes")
                || (closed.side == "NO" && actual == "No");
            let pnl = closed.realized_pnl.unwrap_or(0.0);
            summary.settled += 1;
            if won {
                summary.wins += 1;
            } else {
                summary.losses += 1;
            }
            summary.total_pnl += pnl;

            // Sync predictions journal (Win/Loss + PnL) so Portfolio cards match paper lots.
            let (pred_id, pred_outcome) =
                match sync_prediction_from_settled_lot(pool, &closed, &actual, pnl).await {
                    Ok(pair) => pair,
                    Err(e) => {
                        tracing::warn!(
                            "paper settle prediction sync lot {}: {e}",
                            closed.id
                        );
                        (closed.prediction_id.clone(), None)
                    }
                };

            summary.details.push(PaperSettlementDetail {
                lot_id: closed.id,
                ticker: closed.ticker,
                side: closed.side,
                result: actual.clone(),
                realized_pnl: pnl,
                prediction_id: pred_id,
                prediction_outcome: pred_outcome,
            });
        }
    }

    Ok(summary)
}

/// Grade matching prediction row(s) when a paper lot settles.
/// Returns (prediction_id, outcome label) when a row was updated.
pub async fn sync_prediction_from_settled_lot(
    pool: &Pool<Sqlite>,
    lot: &PaperLot,
    actual_yes_no: &str,
    realized_pnl: f64,
) -> Result<(Option<String>, Option<String>), String> {
    let pred_id = resolve_prediction_id_for_lot(pool, lot).await?;
    let Some(pred_id) = pred_id else {
        return Ok((None, None));
    };

    let won = (lot.side.eq_ignore_ascii_case("YES") && actual_yes_no.eq_ignore_ascii_case("yes"))
        || (lot.side.eq_ignore_ascii_case("NO") && actual_yes_no.eq_ignore_ascii_case("no"));
    let outcome = if realized_pnl > 0.0 {
        "Win"
    } else if realized_pnl < 0.0 {
        "Loss"
    } else if won {
        "Win"
    } else {
        "Loss"
    };
    // Held-side settlement value in dollars for CLV-ish close mark.
    let close_price = if won { 1.0 } else { 0.0 };
    let entry_dollars = (lot.entry_price_cents / 100.0).clamp(0.0, 1.0);
    let clv = close_price - entry_dollars;
    let notes = format!(
        "Outcome: {actual_yes_no}, PnL: {realized_pnl:.4} (synced from paper lot {})",
        lot.id
    );
    let resolved_at = Utc::now().to_rfc3339();

    let rows = sqlx::query(
        r#"
        UPDATE predictions
        SET outcome = ?1,
            actual_result = ?2,
            notes = ?3,
            resolved_at = ?4,
            close_price = ?5,
            clv = ?6
        WHERE id = ?7
          AND (outcome IS NULL OR outcome = '' OR outcome = 'Pending')
        "#,
    )
    .bind(outcome)
    .bind(realized_pnl)
    .bind(&notes)
    .bind(&resolved_at)
    .bind(close_price)
    .bind(clv)
    .bind(&pred_id)
    .execute(pool)
    .await
    .map_err(|e| format!("prediction sync update: {e}"))?
    .rows_affected();

    if rows == 0 {
        // Already graded or missing — still report the link.
        return Ok((Some(pred_id), None));
    }
    Ok((Some(pred_id), Some(outcome.to_string())))
}

/// Prefer lot.prediction_id; else latest pending prediction for this ticker.
async fn resolve_prediction_id_for_lot(
    pool: &Pool<Sqlite>,
    lot: &PaperLot,
) -> Result<Option<String>, String> {
    if let Some(ref id) = lot.prediction_id {
        if !id.is_empty() {
            return Ok(Some(id.clone()));
        }
    }
    // Fallback: pending journal rows for this ticker (player_name or decision JSON).
    let row = sqlx::query(
        r#"
        SELECT id FROM predictions
        WHERE (outcome IS NULL OR outcome = '' OR outcome = 'Pending')
          AND (
            player_name = ?1
            OR full_decision_json LIKE ?2
          )
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(&lot.ticker)
    .bind(format!("%\"ticker\":\"{}%", lot.ticker))
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("prediction lookup: {e}"))?;

    Ok(row.map(|r| r.get::<String, _>("id")))
}

// ═══════════════════════════════════════════════════════════════
// Analytics
// ═══════════════════════════════════════════════════════════════

pub async fn get_analytics(
    pool: &Pool<Sqlite>,
    client: Option<&KalshiClient>,
) -> Result<PaperAnalytics, String> {
    let all = get_all_lots(pool).await?;
    let account = get_account(pool).await?;
    let closed: Vec<&PaperLot> = all.iter().filter(|l| l.status == "Closed").collect();
    let open_positions = all.iter().filter(|l| l.status == "Open").count() as u32;

    let realized_pnl: f64 = closed.iter().map(|l| l.realized_pnl.unwrap_or(0.0)).sum();

    let mut wins = 0u32;
    let mut losses = 0u32;
    let mut gross_wins = 0.0;
    let mut gross_losses = 0.0;
    let mut largest_winner: f64 = 0.0;
    let mut largest_loser: f64 = 0.0;

    for l in &closed {
        let pnl = l.realized_pnl.unwrap_or(0.0);
        if pnl > 0.0 {
            wins += 1;
            gross_wins += pnl;
            largest_winner = largest_winner.max(pnl);
        } else if pnl < 0.0 {
            losses += 1;
            gross_losses += pnl.abs();
            largest_loser = largest_loser.min(pnl);
        }
    }

    let win_rate = if wins + losses > 0 {
        (wins as f64 / (wins + losses) as f64) * 100.0
    } else {
        0.0
    };
    // Cap PF so JSON never emits Infinity (breaks serde_json / UI).
    let profit_factor = if gross_losses > 0.0 {
        gross_wins / gross_losses
    } else if gross_wins > 0.0 {
        999.0
    } else {
        0.0
    };

    let avg_winner = if wins > 0 { gross_wins / wins as f64 } else { 0.0 };
    let avg_loser = if losses > 0 { -gross_losses / losses as f64 } else { 0.0 };

    let positions = aggregate_positions(pool, client).await?;
    let open_market_value: f64 = positions
        .iter()
        .map(|p| p.market_value_dollars.unwrap_or(p.cost_basis_dollars))
        .sum();
    let unrealized_pnl: f64 = positions
        .iter()
        .map(|p| p.unrealized_pnl_dollars.unwrap_or(0.0))
        .sum();

    let equity = account.balance_dollars + open_market_value;
    let total_return_pct = if account.total_deposits > 0.0 {
        ((equity - account.total_deposits) / account.total_deposits) * 100.0
    } else {
        0.0
    };

    let max_dd = max_drawdown_pct(pool).await.unwrap_or(0.0);

    Ok(PaperAnalytics {
        starting_balance: account.total_deposits,
        cash_balance: account.balance_dollars,
        open_market_value,
        equity,
        realized_pnl,
        unrealized_pnl,
        total_return_pct,
        total_trades: all.len() as u32,
        open_positions,
        win_rate,
        wins,
        losses,
        profit_factor,
        avg_winner,
        avg_loser,
        largest_winner,
        largest_loser,
        max_drawdown_pct: max_dd,
        fetched_at: Utc::now().to_rfc3339(),
    })
}

async fn max_drawdown_pct(pool: &Pool<Sqlite>) -> Result<f64, String> {
    let rows = sqlx::query(
        "SELECT equity_dollars FROM paper_equity_snapshots ORDER BY ts ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch equity snapshots: {}", e))?;

    if rows.is_empty() {
        return Ok(0.0);
    }

    let mut peak = 0.0;
    let mut max_dd = 0.0;
    for row in rows {
        let equity: f64 = row.get("equity_dollars");
        if equity > peak {
            peak = equity;
        }
        if peak > 0.0 {
            let dd = (peak - equity) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }

    Ok(max_dd * 100.0)
}

// ═══════════════════════════════════════════════════════════════
// Equity snapshots
// ═══════════════════════════════════════════════════════════════

pub async fn record_equity_snapshot(
    pool: &Pool<Sqlite>,
    client: Option<&KalshiClient>,
) -> Result<(), String> {
    let account = get_account(pool).await?;
    let positions = aggregate_positions(pool, client).await?;
    // Sprint 0.3: never treat open inventory as $0 MV when marks are missing —
    // fall back to cost basis so post-open equity ≈ cash + open cost (not fake DD).
    let open_market_value: f64 = positions
        .iter()
        .map(|p| p.market_value_dollars.unwrap_or(p.cost_basis_dollars))
        .sum();
    let unrealized: f64 = positions
        .iter()
        .map(|p| {
            p.unrealized_pnl_dollars.unwrap_or_else(|| {
                // When MV fell back to cost basis, unrealized is ~0 before fees nuance.
                0.0
            })
        })
        .sum();
    let equity = account.balance_dollars + open_market_value;

    sqlx::query(
        "INSERT INTO paper_equity_snapshots (ts, balance_dollars, open_market_value, equity_dollars, unrealized_pnl) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(Utc::now().to_rfc3339())
    .bind(account.balance_dollars)
    .bind(open_market_value)
    .bind(equity)
    .bind(unrealized)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to record equity snapshot: {}", e))?;

    Ok(())
}

pub async fn get_equity_snapshots(
    pool: &Pool<Sqlite>,
    limit: i64,
) -> Result<Vec<PaperEquitySnapshot>, String> {
    let rows = sqlx::query(
        "SELECT id, ts, balance_dollars, open_market_value, equity_dollars, unrealized_pnl FROM paper_equity_snapshots ORDER BY ts DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch equity snapshots: {}", e))?;

    Ok(rows
        .iter()
        .map(|r| PaperEquitySnapshot {
            id: r.get("id"),
            ts: r.get("ts"),
            balance_dollars: r.get("balance_dollars"),
            open_market_value: r.get("open_market_value"),
            equity_dollars: r.get("equity_dollars"),
            unrealized_pnl: r.get("unrealized_pnl"),
        })
        .collect())
}

// ═══════════════════════════════════════════════════════════════
// Mark-to-market helpers
// ═══════════════════════════════════════════════════════════════

async fn best_bid_cents(
    client: &KalshiClient,
    ticker: &str,
    side: &str,
) -> Result<f64, String> {
    let book = client.fetch_orderbook(ticker).await?;
    let best = match side {
        "YES" => best_bid(&book.yes),
        "NO" => best_bid(&book.no),
        _ => None,
    };

    if let Some(price) = best {
        return Ok(price);
    }

    // Fallback to last traded price.
    let market = client.fetch_market(ticker).await?;
    let last: f64 = market.last_price_dollars.parse().unwrap_or(0.0);
    if last <= 0.0 {
        return Err(format!("No mark price available for {}", ticker));
    }

    let cents = last * 100.0;
    match side {
        "YES" => Ok(cents),
        "NO" => Ok((100.0 - cents).clamp(0.0, 100.0)),
        _ => Err(format!("Invalid side for mark: {}", side)),
    }
}

fn best_bid(levels: &[crate::kalshi::models::KalshiOrderbookLevel]) -> Option<f64> {
    levels
        .iter()
        .map(|l| l.price as f64)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

// ═══════════════════════════════════════════════════════════════
// Background settlement
// ═══════════════════════════════════════════════════════════════

pub fn spawn_paper_settle_task(
    pool: Pool<Sqlite>,
    kalshi: std::sync::Arc<KalshiClient>,
    poll_interval_secs: u64,
) {
    let interval_secs = poll_interval_secs.max(60);
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let open_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM paper_lots WHERE status = 'Open'",
            )
            .fetch_one(&pool)
            .await
            .unwrap_or(0);
            if open_count == 0 {
                continue;
            }
            let summary = match settle_pending(&pool, &*kalshi).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("paper auto-settle: {}", e);
                    continue;
                }
            };
            if summary.settled > 0 {
                tracing::info!(
                    "paper auto-settle: {} lots ({}W/{}L, ${:.2})",
                    summary.settled,
                    summary.wins,
                    summary.losses,
                    summary.total_pnl
                );
            }
        }
    });
}

/// Normalize a dollar or cent price into Kalshi cents (0–100).
pub fn normalize_entry_cents(price: f64) -> f64 {
    if price > 0.0 && price < 1.0 {
        price * 100.0
    } else {
        price.clamp(0.01, 99.99)
    }
}

/// Current drawdown from equity high-water mark as a fraction (0..1).
pub async fn current_drawdown_fraction(pool: &Pool<Sqlite>) -> Result<f64, String> {
    let rows = sqlx::query("SELECT equity_dollars FROM paper_equity_snapshots ORDER BY ts ASC")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch equity snapshots: {e}"))?;
    if rows.is_empty() {
        return Ok(0.0);
    }
    let mut peak = 0.0f64;
    for row in &rows {
        let eq: f64 = row.get("equity_dollars");
        if eq > peak {
            peak = eq;
        }
    }
    let last: f64 = rows.last().unwrap().get("equity_dollars");
    if peak <= 0.0 {
        return Ok(0.0);
    }
    Ok(((peak - last) / peak).max(0.0))
}

/// Realized loss today as a fraction of deposits (positive = loss day).
pub async fn daily_realized_loss_fraction(pool: &Pool<Sqlite>) -> Result<f64, String> {
    let account = get_account(pool).await?;
    let base = account.total_deposits.max(1.0);
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let row = sqlx::query(
        "SELECT COALESCE(SUM(realized_pnl), 0) AS pnl FROM paper_lots WHERE closed_at IS NOT NULL AND closed_at LIKE ?1 || '%'",
    )
    .bind(&today)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to sum daily paper PnL: {e}"))?;
    let pnl: f64 = row.get("pnl");
    if pnl >= 0.0 {
        Ok(0.0)
    } else {
        Ok((-pnl / base).max(0.0))
    }
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_yes_payout_math() {
        let qty = 10.0;
        let entry = 55.0; // cents
        let cost = qty * entry / 100.0; // $5.50
        let fee = order_fee(entry / 100.0, qty, EdgeConfig::default().fee_multiplier); // ≈$0.17
        let total_cost = cost + fee;
        let exit = 100.0;
        let proceeds = qty * exit / 100.0; // $10.00
        let pnl: f64 = proceeds - total_cost;
        assert!((pnl - 4.33).abs() < 0.02, "expected ~4.33, got {pnl}");
    }

    #[test]
    fn long_no_payout_math() {
        let qty = 10.0;
        let entry = 45.0; // NO price in cents
        let cost = qty * entry / 100.0; // $4.50
        let fee = order_fee(entry / 100.0, qty, EdgeConfig::default().fee_multiplier); // ≈$0.17
        let total_cost = cost + fee;
        let exit = 100.0; // No wins
        let proceeds = qty * exit / 100.0; // $10.00
        let pnl: f64 = proceeds - total_cost;
        assert!((pnl - 5.33).abs() < 0.02, "expected ~5.33, got {pnl}");
    }

    #[test]
    fn normalize_entry_cents_from_dollars() {
        assert!((normalize_entry_cents(0.55) - 55.0).abs() < 0.001);
        assert!((normalize_entry_cents(55.0) - 55.0).abs() < 0.001);
    }

    #[test]
    fn source_roundtrip() {
        assert_eq!(
            PaperTradeSource::AiDecision,
            "AiDecision".parse().unwrap()
        );
        assert!("Other".parse::<PaperTradeSource>().is_err());
    }

    #[tokio::test]
    async fn place_trade_is_atomic_and_enforces_balance() {
        let pool = Pool::<Sqlite>::connect("sqlite::memory:")
            .await
            .unwrap();
        init_paper_tables(&pool).await.unwrap();

        let input = PaperTradeInput {
            ticker: "KXTEST-1".into(),
            title: "Test market".into(),
            category: "Economics".into(),
            side: "YES".into(),
            qty: 10_000.0,            // 10,000 contracts @ 55c = $5,500 + fees
            entry_price_cents: 55.0,
            source: PaperTradeSource::AiDecision,
            decision_json: None,
            prediction_id: None,
        };
        let total_cost = 5500.0 + order_fee(0.55, 10_000.0, EdgeConfig::default().fee_multiplier);

        // First trade fits inside the $10,000 starting balance.
        let lot = place_trade(&pool, input.clone()).await.unwrap();
        assert_eq!(lot.status, "Open");
        assert!((lot.stake_dollars - total_cost).abs() < 0.001);

        let account = get_account(&pool).await.unwrap();
        assert!((account.balance_dollars - (10_000.0 - total_cost)).abs() < 0.001);

        // Second identical trade should fail (would need total_cost, only remainder left).
        let err = place_trade(&pool, input.clone()).await.unwrap_err();
        assert!(err.contains("Insufficient paper buying power"), "expected insufficient funds, got: {}", err);

        // Balance and lot count must reflect exactly one successful trade.
        let account = get_account(&pool).await.unwrap();
        assert!((account.balance_dollars - (10_000.0 - total_cost)).abs() < 0.001);
        let lots = get_all_lots(&pool).await.unwrap();
        assert_eq!(lots.len(), 1);
    }

    #[tokio::test]
    async fn concurrent_place_trades_do_not_double_spend() {
        let pool = Pool::<Sqlite>::connect("sqlite::memory:")
            .await
            .unwrap();
        init_paper_tables(&pool).await.unwrap();

        let input = PaperTradeInput {
            ticker: "KXTEST-2".into(),
            title: "Test market".into(),
            category: "Economics".into(),
            side: "YES".into(),
            qty: 10_000.0,            // ~$5,673 each; two would overdraw
            entry_price_cents: 55.0,
            source: PaperTradeSource::AiDecision,
            decision_json: None,
            prediction_id: None,
        };
        let total_cost = 5500.0 + order_fee(0.55, 10_000.0, EdgeConfig::default().fee_multiplier);

        // Both trades individually fit but together would overdraw. Run them
        // concurrently on cloned pools to exercise the balance-check race path.
        let pool_a = pool.clone();
        let pool_b = pool.clone();
        let input_a = input.clone();
        let input_b = input.clone();
        let h1 = tokio::spawn(async move { place_trade(&pool_a, input_a).await });
        let h2 = tokio::spawn(async move { place_trade(&pool_b, input_b).await });
        let (a, b) = (h1.await.unwrap(), h2.await.unwrap());

        let successes = [a.is_ok(), b.is_ok()].into_iter().filter(|x| *x).count();
        assert!(
            successes <= 1,
            "only one of two concurrent trades may succeed, got {}",
            successes
        );

        let account = get_account(&pool).await.unwrap();
        let lots = get_all_lots(&pool).await.unwrap();
        let expected_balance = if successes == 1 { 10_000.0 - total_cost } else { 10000.0 };
        assert!((account.balance_dollars - expected_balance).abs() < 0.001);
        assert_eq!(lots.len(), successes);
    }

    #[tokio::test]
    async fn equity_snapshot_uses_cost_basis_when_no_marks() {
        let pool = Pool::<Sqlite>::connect("sqlite::memory:").await.unwrap();
        init_paper_tables(&pool).await.unwrap();
        let input = PaperTradeInput {
            ticker: "KXTEST-EQ".into(),
            title: "Eq".into(),
            category: "Other".into(),
            side: "YES".into(),
            qty: 100.0,
            entry_price_cents: 50.0,
            source: PaperTradeSource::Manual,
            decision_json: None,
            prediction_id: None,
        };
        let lot = place_trade(&pool, input).await.unwrap();
        // place_trade already recorded a snapshot with cost-basis fallback
        let snaps = get_equity_snapshots(&pool, 5).await.unwrap();
        assert!(!snaps.is_empty());
        let s = &snaps[0];
        // Cash debited by stake; open MV should be ~cost basis, not zero
        assert!(
            s.open_market_value > 0.0,
            "expected cost-basis open MV > 0, got {}",
            s.open_market_value
        );
        // Equity ≈ starting balance minus fee (not cash-only crash)
        assert!(
            s.equity_dollars > s.balance_dollars,
            "equity {} should exceed cash {} when open inventory valued",
            s.equity_dollars,
            s.balance_dollars
        );
        assert!(lot.stake_dollars > 0.0);
    }

    #[test]
    fn settlement_exit_cents_side_aware() {
        // YES holder wins only when result is Yes
        assert!((settlement_exit_cents_for_side("YES", "Yes") - 100.0).abs() < 1e-9);
        assert!((settlement_exit_cents_for_side("YES", "No") - 0.0).abs() < 1e-9);
        // NO holder wins when result is No — previously inverted (used YES exit only)
        assert!((settlement_exit_cents_for_side("NO", "No") - 100.0).abs() < 1e-9);
        assert!((settlement_exit_cents_for_side("NO", "Yes") - 0.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn no_side_settlement_pays_on_no_result() {
        let pool = Pool::<Sqlite>::connect("sqlite::memory:").await.unwrap();
        init_paper_tables(&pool).await.unwrap();
        // Buy NO at 34c (like shorting YES at 66)
        let input = PaperTradeInput {
            ticker: "KXTEST-NO".into(),
            title: "No side".into(),
            category: "Politics".into(),
            side: "NO".into(),
            qty: 100.0,
            entry_price_cents: 34.0,
            source: PaperTradeSource::AiDecision,
            decision_json: None,
            prediction_id: None,
        };
        let lot = place_trade(&pool, input).await.unwrap();
        let exit = settlement_exit_cents_for_side(&lot.side, "No");
        let closed = close_lot_with_result(&pool, &lot.id, exit, "No")
            .await
            .unwrap();
        // Win: 100 contracts * $1 - stake (34 + fee)
        assert!(
            closed.realized_pnl.unwrap_or(0.0) > 0.0,
            "NO winner should have positive PnL, got {:?}",
            closed.realized_pnl
        );
        assert_eq!(closed.settlement_result.as_deref(), Some("No"));
    }

    /// Simulated Analyst TAKE → open lot → settle YES → grade quality rating.
    #[tokio::test]
    async fn analyst_style_take_settle_and_rate() {
        let pool = Pool::<Sqlite>::connect("sqlite::memory:").await.unwrap();
        init_paper_tables(&pool).await.unwrap();
        // Prompt-style decision: YES on a 40¢ market with fair 55% (edge claim)
        let input = PaperTradeInput {
            ticker: "KXBTCD-26JUL15-B100000".into(),
            title: "Will Bitcoin be above $100,000?".into(),
            category: "Crypto".into(),
            side: "YES".into(),
            qty: 50.0,
            entry_price_cents: 40.0,
            source: PaperTradeSource::AiDecision,
            decision_json: Some(
                r#"{"ticker":"KXBTCD-26JUL15-B100000","decision":"TAKE","contract_side":"YES","fair_probability_pct":55,"market_price_pct":40,"recommended_stake_dollars":20}"#.into(),
            ),
            prediction_id: Some("pred-analyst-sim-1".into()),
        };
        let lot = place_trade(&pool, input).await.unwrap();
        assert!(lot.stake_dollars > 0.0);
        // Market resolves YES → analyst was correct direction
        let closed = close_lot_with_result(
            &pool,
            &lot.id,
            settlement_exit_cents_for_side("YES", "Yes"),
            "Yes",
        )
        .await
        .unwrap();
        let pnl = closed.realized_pnl.unwrap_or(0.0);
        assert!(pnl > 0.0, "winning YES TAKE should profit, pnl={pnl}");
        let snaps = get_equity_snapshots(&pool, 5).await.unwrap();
        assert!(!snaps.is_empty());
        let a = get_analytics(&pool, None).await.unwrap();
        assert!(a.wins >= 1);
        assert!(a.profit_factor.is_finite());
        assert!(a.profit_factor > 0.0 && a.profit_factor <= 999.0);
        // Simple prediction quality rating (process + outcome)
        // Edge claimed 15pts, won → strong outcome; process still depends on rails.
        let rating = if pnl > 0.0 { "B+" } else { "D" };
        assert_eq!(rating, "B+");
    }

    #[tokio::test]
    async fn settle_syncs_linked_prediction_outcome() {
        let pool = Pool::<Sqlite>::connect("sqlite::memory:").await.unwrap();
        init_paper_tables(&pool).await.unwrap();
        // Minimal predictions table for sync path
        sqlx::query(
            r#"
            CREATE TABLE predictions (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                raw_text TEXT,
                player_name TEXT,
                pick_type TEXT,
                line REAL,
                stat_category TEXT,
                confidence TEXT,
                confidence_score INTEGER,
                probability REAL,
                reasoning TEXT,
                risk TEXT,
                created_at TEXT,
                outcome TEXT,
                actual_result REAL,
                notes TEXT,
                resolved_at TEXT,
                full_decision_json TEXT,
                entry_price REAL,
                close_price REAL,
                clv REAL,
                model_disagreement INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS forecasts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                market_ticker TEXT NOT NULL,
                created_at TEXT NOT NULL,
                close_time TEXT NOT NULL,
                p_market REAL NOT NULL,
                p_model REAL,
                p_final REAL NOT NULL,
                verdict TEXT NOT NULL,
                verdict_reasons TEXT NOT NULL,
                stake_suggested REAL,
                agent_breakdown TEXT,
                resolved_at TEXT,
                outcome INTEGER,
                brier_model REAL,
                brier_market REAL,
                brier_final REAL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let pred_id = "pred-sync-1";
        sqlx::query(
            "INSERT INTO predictions (id, session_id, raw_text, player_name, created_at, outcome, entry_price, full_decision_json)
             VALUES (?1, 's', '{}', 'KXTEST-SYNC', datetime('now'), 'Pending', 0.40, '{\"ticker\":\"KXTEST-SYNC\"}')",
        )
        .bind(pred_id)
        .execute(&pool)
        .await
        .unwrap();

        let input = PaperTradeInput {
            ticker: "KXTEST-SYNC".into(),
            title: "Sync test".into(),
            category: "Politics".into(),
            side: "YES".into(),
            qty: 10.0,
            entry_price_cents: 40.0,
            source: PaperTradeSource::AiDecision,
            decision_json: Some(r#"{"ticker":"KXTEST-SYNC"}"#.into()),
            prediction_id: Some(pred_id.into()),
        };
        let lot = place_trade(&pool, input).await.unwrap();
        assert_eq!(lot.prediction_id.as_deref(), Some(pred_id));

        // Simulate settle path for YES winner
        let exit = settlement_exit_cents_for_side("YES", "Yes");
        let closed = close_lot_with_result(&pool, &lot.id, exit, "Yes")
            .await
            .unwrap();
        let pnl = closed.realized_pnl.unwrap_or(0.0);
        let (synced_id, outcome) =
            sync_prediction_from_settled_lot(&pool, &closed, "Yes", pnl)
                .await
                .unwrap();
        assert_eq!(synced_id.as_deref(), Some(pred_id));
        assert_eq!(outcome.as_deref(), Some("Win"));

        let row = sqlx::query("SELECT outcome, actual_result FROM predictions WHERE id = ?1")
            .bind(pred_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let out: String = row.get("outcome");
        let ar: f64 = row.get("actual_result");
        assert_eq!(out, "Win");
        assert!((ar - pnl).abs() < 1e-6);
    }

    #[tokio::test]
    async fn close_lot_realized_pnl_includes_entry_fees() {
        let pool = Pool::<Sqlite>::connect("sqlite::memory:")
            .await
            .unwrap();
        init_paper_tables(&pool).await.unwrap();

        let input = PaperTradeInput {
            ticker: "KXTEST-3".into(),
            title: "Test market".into(),
            category: "Economics".into(),
            side: "YES".into(),
            qty: 100.0,
            entry_price_cents: 50.0,
            source: PaperTradeSource::Manual,
            decision_json: None,
            prediction_id: None,
        };
        let lot = place_trade(&pool, input).await.unwrap();
        let total_cost = lot.stake_dollars;
        let fee = order_fee(0.50, 100.0, EdgeConfig::default().fee_multiplier);
        assert!((total_cost - (50.0 + fee)).abs() < 0.001);

        let closed = close_lot(&pool, &lot.id, 100.0).await.unwrap();
        let realized = closed.realized_pnl.unwrap();
        // Proceeds $100 - total cost (gross $50 + fee).
        assert!((realized - (50.0 - fee)).abs() < 0.001, "expected {}, got {}", 50.0 - fee, realized);
    }
}
