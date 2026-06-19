//! Kalshi market price / spread snapshot history.

use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};

use super::models::KalshiMarketSummary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiPriceSnapshot {
    pub id: String,
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub yes_prob_pct: f64,
    pub yes_bid: f64,
    pub yes_ask: f64,
    pub spread: f64,
    pub volume_24h: f64,
    pub liquidity: f64,
    pub snapshot_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiPriceHistory {
    pub ticker: String,
    pub snapshots: Vec<KalshiPriceSnapshot>,
    pub opening_yes_prob: Option<f64>,
    pub current_yes_prob: Option<f64>,
    pub prob_change: Option<f64>,
    pub spread_change: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiSnapshotBatch {
    pub snapshots_taken: usize,
    pub snapshot_at: String,
}

pub async fn init_price_tables(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS kalshi_price_snapshots (
            id TEXT PRIMARY KEY,
            ticker TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            category TEXT NOT NULL DEFAULT '',
            yes_prob_pct REAL NOT NULL,
            yes_bid REAL NOT NULL,
            yes_ask REAL NOT NULL,
            spread REAL NOT NULL,
            volume_24h REAL NOT NULL DEFAULT 0,
            liquidity REAL NOT NULL DEFAULT 0,
            snapshot_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("kalshi_price_snapshots create failed: {}", e))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_kps_ticker ON kalshi_price_snapshots(ticker)")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_kps_snapshot ON kalshi_price_snapshots(snapshot_at)")
        .execute(pool)
        .await
        .ok();

    Ok(())
}

pub async fn snapshot_markets(
    pool: &Pool<Sqlite>,
    markets: &[KalshiMarketSummary],
) -> Result<KalshiSnapshotBatch, String> {
    let snapshot_at = chrono::Utc::now().to_rfc3339();
    let mut count = 0usize;

    for m in markets {
        let id = format!("{}-{}", m.ticker, snapshot_at);
        let rows = sqlx::query(
            r#"
            INSERT OR IGNORE INTO kalshi_price_snapshots
                (id, ticker, title, category, yes_prob_pct, yes_bid, yes_ask, spread,
                 volume_24h, liquidity, snapshot_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
        )
        .bind(&id)
        .bind(&m.ticker)
        .bind(&m.title)
        .bind(&m.category)
        .bind(m.yes_prob_pct)
        .bind(m.yes_bid)
        .bind(m.yes_ask)
        .bind(m.spread)
        .bind(m.volume_24h)
        .bind(m.liquidity)
        .bind(&snapshot_at)
        .execute(pool)
        .await
        .map_err(|e| format!("insert kalshi snapshot: {}", e))?
        .rows_affected();

        if rows > 0 {
            count += 1;
        }
    }

    Ok(KalshiSnapshotBatch {
        snapshots_taken: count,
        snapshot_at,
    })
}

pub async fn get_price_history(
    pool: &Pool<Sqlite>,
    ticker: &str,
    limit: i64,
) -> Result<KalshiPriceHistory, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, ticker, title, category, yes_prob_pct, yes_bid, yes_ask, spread,
               volume_24h, liquidity, snapshot_at
        FROM kalshi_price_snapshots
        WHERE ticker = ?1
        ORDER BY snapshot_at ASC
        LIMIT ?2
        "#,
    )
    .bind(ticker)
    .bind(limit.max(2))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("fetch kalshi price history: {}", e))?;

    let snapshots: Vec<KalshiPriceSnapshot> = rows
        .iter()
        .map(|r| KalshiPriceSnapshot {
            id: r.get("id"),
            ticker: r.get("ticker"),
            title: r.get("title"),
            category: r.get("category"),
            yes_prob_pct: r.get("yes_prob_pct"),
            yes_bid: r.get("yes_bid"),
            yes_ask: r.get("yes_ask"),
            spread: r.get("spread"),
            volume_24h: r.get("volume_24h"),
            liquidity: r.get("liquidity"),
            snapshot_at: r.get("snapshot_at"),
        })
        .collect();

    let opening_yes_prob = snapshots.first().map(|s| s.yes_prob_pct);
    let current_yes_prob = snapshots.last().map(|s| s.yes_prob_pct);
    let prob_change = match (opening_yes_prob, current_yes_prob) {
        (Some(a), Some(b)) => Some(b - a),
        _ => None,
    };
    let spread_change = match (snapshots.first(), snapshots.last()) {
        (Some(a), Some(b)) => Some(b.spread - a.spread),
        _ => None,
    };

    Ok(KalshiPriceHistory {
        ticker: ticker.to_string(),
        snapshots,
        opening_yes_prob,
        current_yes_prob,
        prob_change,
        spread_change,
    })
}

pub async fn prune_old_snapshots(pool: &Pool<Sqlite>, keep_days: i64) -> Result<u64, String> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(keep_days.max(1));
    let rows = sqlx::query("DELETE FROM kalshi_price_snapshots WHERE snapshot_at < ?1")
        .bind(cutoff.to_rfc3339())
        .execute(pool)
        .await
        .map_err(|e| format!("prune kalshi snapshots: {}", e))?
        .rows_affected();
    Ok(rows)
}