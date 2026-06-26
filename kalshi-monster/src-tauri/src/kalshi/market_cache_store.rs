//! SQLite persistence for the Kalshi market list cache (instant dashboard paint on next launch).

use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::time::{SystemTime, UNIX_EPOCH};

use super::models::{KalshiCache, KalshiMarket};

const CACHE_ROW_ID: &str = "default";

/// Rehydrate in-memory cache at startup if persisted data is newer than this (API refresh still uses CACHE_TTL).
pub const PERSISTED_REHYDRATE_MAX_AGE_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedKalshiCachePayload {
    markets: Vec<KalshiMarket>,
    fetched_at: u64,
    full_catalog: bool,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub async fn init_market_cache_table(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS kalshi_market_cache (
            id TEXT PRIMARY KEY,
            payload_json TEXT NOT NULL,
            market_count INTEGER NOT NULL DEFAULT 0,
            fetched_at INTEGER NOT NULL,
            full_catalog INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("kalshi_market_cache create failed: {}", e))?;

    Ok(())
}

pub async fn load_persisted_cache(pool: &Pool<Sqlite>) -> Result<Option<KalshiCache>, String> {
    let row = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT payload_json, fetched_at, full_catalog FROM kalshi_market_cache WHERE id = ?",
    )
    .bind(CACHE_ROW_ID)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("kalshi_market_cache load failed: {}", e))?;

    let Some((payload_json, fetched_at, full_catalog)) = row else {
        return Ok(None);
    };

    let age = now_secs().saturating_sub(fetched_at as u64);
    if age > PERSISTED_REHYDRATE_MAX_AGE_SECS {
        tracing::info!(
            "kalshi persisted market cache too old ({}s); skipping rehydrate",
            age
        );
        return Ok(None);
    }

    let payload: PersistedKalshiCachePayload = serde_json::from_str(&payload_json)
        .map_err(|e| format!("kalshi_market_cache JSON parse failed: {}", e))?;

    Ok(Some(KalshiCache {
        markets: payload.markets,
        fetched_at: fetched_at as u64,
        full_catalog: full_catalog != 0,
    }))
}

pub async fn save_persisted_cache(pool: &Pool<Sqlite>, cache: &KalshiCache) -> Result<(), String> {
    let payload = PersistedKalshiCachePayload {
        markets: cache.markets.clone(),
        fetched_at: cache.fetched_at,
        full_catalog: cache.full_catalog,
    };
    let payload_json = serde_json::to_string(&payload)
        .map_err(|e| format!("kalshi_market_cache JSON encode failed: {}", e))?;
    let updated_at = chrono::Utc::now().to_rfc3339();
    let full_catalog = if cache.full_catalog { 1i64 } else { 0 };

    sqlx::query(
        r#"
        INSERT INTO kalshi_market_cache (id, payload_json, market_count, fetched_at, full_catalog, updated_at)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            payload_json = excluded.payload_json,
            market_count = excluded.market_count,
            fetched_at = excluded.fetched_at,
            full_catalog = excluded.full_catalog,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(CACHE_ROW_ID)
    .bind(&payload_json)
    .bind(cache.markets.len() as i64)
    .bind(cache.fetched_at as i64)
    .bind(full_catalog)
    .bind(&updated_at)
    .execute(pool)
    .await
    .map_err(|e| format!("kalshi_market_cache save failed: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_payload_roundtrip() {
        let cache = KalshiCache {
            markets: vec![KalshiMarket {
                ticker: "TEST-TICKER".to_string(),
                event_ticker: "TEST-EVENT".to_string(),
                title: "Test market".to_string(),
                ..Default::default()
            }],
            fetched_at: 1_700_000_000,
            full_catalog: false,
        };
        let payload = PersistedKalshiCachePayload {
            markets: cache.markets.clone(),
            fetched_at: cache.fetched_at,
            full_catalog: cache.full_catalog,
        };
        let json = serde_json::to_string(&payload).unwrap();
        let back: PersistedKalshiCachePayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back.markets[0].ticker, "TEST-TICKER");
        assert_eq!(back.fetched_at, cache.fetched_at);
        assert!(!back.full_catalog);
    }
}