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


/// Series the Fincept technical agent can price (yfinance underlyings).
/// Snapshotting the full catalog floods the DB with sports/politics while
/// leaving crypto hourlies (KXBTC/KXETH/…) blind — contract_tape then has
/// zero history on the book that matters for edge measurement.
pub const PREFERRED_SNAPSHOT_SERIES: &[&str] = &[
    "KXBTC",
    "KXETH",
    "KXINX",
    "KXNASDAQ100",
    "KXNDX",
    "KXGOLD",
    "KXWTI",
    "KXAAPL",
    "KXTSLA",
    "KXNVDA",
];

/// Default poll interval for preferred-series tape snapshots (seconds).
pub const PREFERRED_SNAPSHOT_INTERVAL_SECS: u64 = 120;

/// True when a market ticker belongs to a preferred series prefix.
pub fn is_preferred_series_ticker(ticker: &str) -> bool {
    let t = ticker.trim().to_ascii_uppercase();
    PREFERRED_SNAPSHOT_SERIES.iter().any(|s| {
        t == *s
            || t.starts_with(&format!("{s}-"))
            // Long-dated series without hyphen after prefix (e.g. KXBTCMAXY-…)
            || (t.starts_with(s)
                && t.len() > s.len()
                && !t.as_bytes()
                    .get(s.len())
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false))
    })
}

/// Fetch open markets for preferred series and write price snapshots.
/// Public so the startup warm path and tests can call it directly.
pub async fn snapshot_preferred_series(
    client: &crate::kalshi::KalshiClient,
    pool: &Pool<Sqlite>,
) -> Result<KalshiSnapshotBatch, String> {
    let markets = client.fetch_preferred_series_markets(100).await?;
    let summaries: Vec<KalshiMarketSummary> =
        markets.iter().map(KalshiMarketSummary::from).collect();
    let batch = snapshot_markets(pool, &summaries).await?;
    tracing::info!(
        "kalshi preferred-series snapshot: {} markets → {} rows at {}",
        summaries.len(),
        batch.snapshots_taken,
        batch.snapshot_at
    );
    Ok(batch)
}

/// Background task: keep contract-tape history warm on agent-priced series.
///
/// Uses `tauri::async_runtime::spawn` (not bare `tokio::spawn`) so the task
/// shares Tauri's reactor — dual-runtime bare spawns were a KB-1 failure mode.
pub fn spawn_preferred_series_snapshot_task(
    client: std::sync::Arc<crate::kalshi::KalshiClient>,
    pool: Pool<Sqlite>,
    poll_interval_secs: u64,
) {
    let interval_secs = poll_interval_secs.max(60);
    tauri::async_runtime::spawn(async move {
        // Initial delay so startup quick-cache / full warm can claim bandwidth first.
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            match snapshot_preferred_series(&client, &pool).await {
                Ok(batch) => {
                    tracing::debug!(
                        "preferred snapshot ok: taken={} at={}",
                        batch.snapshots_taken,
                        batch.snapshot_at
                    );
                }
                Err(e) => {
                    tracing::warn!("preferred-series price snapshot failed: {e}");
                }
            }
            // Prune occasionally so the preferred flood doesn't bloat forever.
            if let Err(e) = prune_old_snapshots(&pool, 14).await {
                tracing::debug!("preferred snapshot prune: {e}");
            }
            ticker.tick().await;
        }
    });
}

#[cfg(test)]
mod preferred_series_tests {
    use super::*;

    #[test]
    fn preferred_series_covers_crypto_and_index_prefixes() {
        assert!(PREFERRED_SNAPSHOT_SERIES.contains(&"KXBTC"));
        assert!(PREFERRED_SNAPSHOT_SERIES.contains(&"KXETH"));
        assert!(PREFERRED_SNAPSHOT_SERIES.contains(&"KXINX"));
        assert!(PREFERRED_SNAPSHOT_SERIES.contains(&"KXNASDAQ100"));
        assert!(is_preferred_series_ticker("KXBTC-26JUL2117-B65375"));
        assert!(is_preferred_series_ticker("KXETH-26JUL2117-B1830"));
        assert!(is_preferred_series_ticker("KXINX-26JUL21H1600-B7487"));
        assert!(is_preferred_series_ticker(
            "KXNASDAQ100-26JUL24H1600-T27600"
        ));
        assert!(!is_preferred_series_ticker("KXMIDTERMMOVE-26-D"));
        assert!(!is_preferred_series_ticker("KXNFLWINS-27-NE"));
    }
}
