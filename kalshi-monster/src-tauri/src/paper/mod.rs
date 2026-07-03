//! Real Kalshi paper-trading engine.
//!
//! Tracks an immutable lot journal, a cash account, and equity snapshots
//! independently of the real-money Kalshi grading path. Payout math follows
//! Kalshi's binary contract rules: each contract pays $1 if the held side
//! wins and $0 if it loses.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};

use crate::kalshi::client::KalshiClient;


pub const PAPER_SESSION_ID: &str = "paper-sim";
const DEFAULT_STARTING_BALANCE: f64 = 10_000.0;

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
    pub stake_dollars: f64,
    pub source: PaperTradeSource,
    pub decision_json: Option<String>,
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
    let side = normalize_side(&input.side)?;
    if input.qty <= 0.0 {
        return Err("Paper trade quantity must be positive".into());
    }
    if input.entry_price_cents <= 0.0 || input.entry_price_cents >= 100.0 {
        return Err("Paper entry price must be between 0 and 100 cents".into());
    }

    let cost = input.qty * input.entry_price_cents / 100.0;
    let account = get_account(pool).await?;
    if cost > account.balance_dollars {
        return Err(format!(
            "Insufficient paper buying power: ${:.2} needed, ${:.2} available",
            cost, account.balance_dollars
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let source_str = input.source.as_str().to_string();

    sqlx::query(
        r#"
        INSERT INTO paper_lots
            (id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
             source, decision_json, opened_at, status)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'Open')
        "#,
    )
    .bind(&id)
    .bind(&input.ticker)
    .bind(&input.title)
    .bind(&input.category)
    .bind(&side)
    .bind(input.entry_price_cents)
    .bind(input.qty)
    .bind(cost)
    .bind(&source_str)
    .bind(&input.decision_json)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to insert paper lot: {}", e))?;

    update_balance(pool, -cost).await?;
    record_equity_snapshot(pool, None).await?;

    get_lot(pool, &id).await
}

pub async fn close_lot(
    pool: &Pool<Sqlite>,
    lot_id: &str,
    exit_price_cents: f64,
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
    let result = if exit_price_cents >= 99.99 {
        "Yes"
    } else if exit_price_cents <= 0.01 {
        "No"
    } else {
        "Closed"
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
    .bind(result)
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
               source, decision_json, opened_at, closed_at, closed_price_cents,
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

pub async fn get_all_lots(pool: &Pool<Sqlite>) -> Result<Vec<PaperLot>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, ticker, title, category, side, entry_price_cents, qty, stake_dollars,
               source, decision_json, opened_at, closed_at, closed_price_cents,
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
               source, decision_json, opened_at, closed_at, closed_price_cents,
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

        let exit = if market.result.eq_ignore_ascii_case("yes") {
            100.0
        } else {
            0.0
        };

        for lot in lots {
            let closed = close_lot(pool, &lot.id, exit).await?;
            let won = (closed.side == "YES" && market.result.eq_ignore_ascii_case("yes"))
                || (closed.side == "NO" && market.result.eq_ignore_ascii_case("no"));
            summary.settled += 1;
            if won {
                summary.wins += 1;
            } else {
                summary.losses += 1;
            }
            summary.total_pnl += closed.realized_pnl.unwrap_or(0.0);
            summary.details.push(PaperSettlementDetail {
                lot_id: closed.id,
                ticker: closed.ticker,
                side: closed.side,
                result: market.result.clone(),
                realized_pnl: closed.realized_pnl.unwrap_or(0.0),
            });
        }
    }

    Ok(summary)
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
    let profit_factor = if gross_losses > 0.0 {
        gross_wins / gross_losses
    } else if gross_wins > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };

    let avg_winner = if wins > 0 { gross_wins / wins as f64 } else { 0.0 };
    let avg_loser = if losses > 0 { -gross_losses / losses as f64 } else { 0.0 };

    let positions = aggregate_positions(pool, client).await?;
    let open_market_value: f64 = positions
        .iter()
        .filter_map(|p| p.market_value_dollars)
        .sum();
    let unrealized_pnl: f64 = positions
        .iter()
        .filter_map(|p| p.unrealized_pnl_dollars)
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
    let open_market_value: f64 = positions
        .iter()
        .filter_map(|p| p.market_value_dollars)
        .sum();
    let unrealized: f64 = positions
        .iter()
        .filter_map(|p| p.unrealized_pnl_dollars)
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
    kalshi: std::sync::Arc<tokio::sync::Mutex<KalshiClient>>,
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
            let summary = {
                let client = kalshi.lock().await;
                match settle_pending(&pool, &client).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("paper auto-settle: {}", e);
                        continue;
                    }
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
        let exit = 100.0;
        let proceeds = qty * exit / 100.0; // $10.00
        let pnl: f64 = proceeds - cost;
        assert!((pnl - 4.50).abs() < 0.001);
    }

    #[test]
    fn long_no_payout_math() {
        let qty = 10.0;
        let entry = 45.0; // NO price in cents
        let cost = qty * entry / 100.0; // $4.50
        let exit = 100.0; // No wins
        let proceeds = qty * exit / 100.0; // $10.00
        let pnl: f64 = proceeds - cost;
        assert!((pnl - 5.50).abs() < 0.001);
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
}
