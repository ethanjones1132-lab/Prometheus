#![allow(dead_code)]
// ═══════════════════════════════════════════════════════════════
// Historical Prop Line Movement Tracking
//
// Tracks how specific player prop lines change over time by
// periodically snapshotting PrizePicks prop data. Enables
// analysis of line movement trends, steam moves, and reverse
// line movement detection.
//
// Schema:
//   line_movements  — snapshots of individual prop lines over time
//   line_summaries  — aggregated per-player/stat movement summaries
// ═══════════════════════════════════════════════════════════════

use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};

// ═══════════════════════════════════════════════════════════════
// Data Types
// ═══════════════════════════════════════════════════════════════

/// A single snapshot of a prop line at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineMovementRecord {
    pub id: String,
    pub player_name: String,
    pub team: String,
    pub opponent: String,
    pub stat_category: String,
    pub league: String,
    pub line: f64,
    pub projection: Option<f64>,
    pub source: String,
    pub game_time: Option<String>,
    pub snapshot_at: String,
    /// Unique key for grouping: "player_name|stat_category|league|game_time"
    pub prop_key: String,
}

/// Aggregated movement summary for a specific prop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineMovementSummary {
    pub prop_key: String,
    pub player_name: String,
    pub team: String,
    pub opponent: String,
    pub stat_category: String,
    pub league: String,
    pub game_time: Option<String>,
    pub current_line: f64,
    pub opening_line: f64,
    pub line_change: f64,
    pub max_line: f64,
    pub min_line: f64,
    pub snapshot_count: i64,
    pub first_seen: String,
    pub last_updated: String,
    /// "up" = line increased, "down" = line decreased, "stable" = no change
    pub direction: String,
    pub projection: Option<f64>,
}

/// Detailed history for a single prop — all snapshots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineDetailHistory {
    pub summary: LineMovementSummary,
    pub snapshots: Vec<LineMovementRecord>,
}

/// Filter options for querying line movements
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LineMovementFilter {
    pub league: Option<String>,
    pub player_name: Option<String>,
    pub stat_category: Option<String>,
    pub direction: Option<String>,   // "up", "down", "stable"
    pub min_change: Option<f64>,     // minimum absolute line change
    pub since: Option<String>,       // ISO 8601 timestamp
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Paginated response for line movement queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineMovementPage {
    pub summaries: Vec<LineMovementSummary>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// Snapshot capture result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotResult {
    pub snapshots_taken: usize,
    pub new_props: usize,
    pub updated_props: usize,
    pub snapshot_at: String,
}

// ═══════════════════════════════════════════════════════════════
// Database Operations
// ═══════════════════════════════════════════════════════════════

/// Ensure the line_movements table exists. Called during db init.
pub async fn init_line_tables(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS line_movements (
            id TEXT PRIMARY KEY,
            player_name TEXT NOT NULL,
            team TEXT NOT NULL DEFAULT '',
            opponent TEXT NOT NULL DEFAULT '',
            stat_category TEXT NOT NULL,
            league TEXT NOT NULL DEFAULT '',
            line REAL NOT NULL,
            projection REAL,
            source TEXT NOT NULL DEFAULT '',
            game_time TEXT,
            snapshot_at TEXT NOT NULL,
            prop_key TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create line_movements table: {}", e))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_lm_prop_key ON line_movements(prop_key)")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_lm_player ON line_movements(player_name)")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_lm_league ON line_movements(league)")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_lm_snapshot ON line_movements(snapshot_at)")
        .execute(pool)
        .await
        .ok();

    Ok(())
}

/// Insert a line movement snapshot. Uses INSERT OR IGNORE to avoid
/// duplicate snapshots for the same prop at the same time.
pub async fn insert_snapshot(
    pool: &Pool<Sqlite>,
    record: &LineMovementRecord,
) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO line_movements
            (id, player_name, team, opponent, stat_category, league,
             line, projection, source, game_time, snapshot_at, prop_key)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        "#,
    )
    .bind(&record.id)
    .bind(&record.player_name)
    .bind(&record.team)
    .bind(&record.opponent)
    .bind(&record.stat_category)
    .bind(&record.league)
    .bind(record.line)
    .bind(record.projection)
    .bind(&record.source)
    .bind(&record.game_time)
    .bind(&record.snapshot_at)
    .bind(&record.prop_key)
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to insert line movement: {}", e))?;

    Ok(())
}

/// Bulk insert snapshots from a PrizePicks data fetch
pub async fn snapshot_props(
    pool: &Pool<Sqlite>,
    props: &[crate::prizepicks::models::PrizePicksProp],
    source: &str,
) -> Result<SnapshotResult, String> {
    let snapshot_at = chrono::Utc::now().to_rfc3339();
    let mut snapshots_taken = 0usize;
    let mut new_props = 0usize;
    let mut updated_props = 0usize;

    for prop in props {
        let prop_key = build_prop_key(
            &prop.player_name,
            &prop.stat_category,
            &prop.league,
            prop.game_time.as_deref(),
        );

        // Check if this prop already exists (to count new vs updated)
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM line_movements WHERE prop_key = ?1",
        )
        .bind(&prop_key)
        .fetch_one(pool)
        .await
        .unwrap_or(Some(0));

        if existing == Some(0) {
            new_props += 1;
        } else {
            updated_props += 1;
        }

        let record = LineMovementRecord {
            id: uuid::Uuid::new_v4().to_string(),
            player_name: prop.player_name.clone(),
            team: prop.team.clone(),
            opponent: prop.opponent.clone(),
            stat_category: prop.stat_category.clone(),
            league: prop.league.clone(),
            line: prop.line,
            projection: prop.projection,
            source: source.to_string(),
            game_time: prop.game_time.clone(),
            snapshot_at: snapshot_at.clone(),
            prop_key: prop_key.clone(),
        };

        insert_snapshot(pool, &record).await?;
        snapshots_taken += 1;
    }

    Ok(SnapshotResult {
        snapshots_taken,
        new_props,
        updated_props,
        snapshot_at,
    })
}

/// Get aggregated line movement summaries with filtering
pub async fn get_line_summaries(
    pool: &Pool<Sqlite>,
    filter: &LineMovementFilter,
) -> Result<LineMovementPage, String> {
    let limit = filter.limit.unwrap_or(50).min(200);
    let offset = filter.offset.unwrap_or(0);

    // Build dynamic WHERE clause
    let mut conditions = Vec::new();
    if filter.league.is_some() {
        conditions.push("lm.league = ?A");
    }
    if filter.player_name.is_some() {
        conditions.push("lm.player_name LIKE ?B");
    }
    if filter.stat_category.is_some() {
        conditions.push("lm.stat_category = ?C");
    }
    if filter.since.is_some() {
        conditions.push("lm.snapshot_at >= ?D");
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Count total
    let count_query = format!(
        "SELECT COUNT(DISTINCT lm.prop_key) FROM line_movements lm {}",
        where_clause
    );
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_query);
    if let Some(ref league) = filter.league {
        count_q = count_q.bind(league);
    }
    if let Some(ref player) = filter.player_name {
        count_q = count_q.bind(format!("%{}%", player));
    }
    if let Some(ref stat) = filter.stat_category {
        count_q = count_q.bind(stat);
    }
    if let Some(ref since) = filter.since {
        count_q = count_q.bind(since);
    }
    let total: i64 = count_q.fetch_one(pool).await.unwrap_or(0);

    // Get summaries using a subquery approach
    let query = r#"
        SELECT
            lm.prop_key,
            lm.player_name,
            lm.team,
            lm.opponent,
            lm.stat_category,
            lm.league,
            lm.game_time,
            (
                SELECT l2.line FROM line_movements l2
                WHERE l2.prop_key = lm.prop_key
                ORDER BY l2.snapshot_at DESC LIMIT 1
            ) as current_line,
            (
                SELECT l3.line FROM line_movements l3
                WHERE l3.prop_key = lm.prop_key
                ORDER BY l3.snapshot_at ASC LIMIT 1
            ) as opening_line,
            (
                SELECT MAX(l4.line) FROM line_movements l4
                WHERE l4.prop_key = lm.prop_key
            ) as max_line,
            (
                SELECT MIN(l5.line) FROM line_movements l5
                WHERE l5.prop_key = lm.prop_key
            ) as min_line,
            COUNT(lm.id) as snapshot_count,
            MIN(lm.snapshot_at) as first_seen,
            MAX(lm.snapshot_at) as last_updated,
            (
                SELECT l6.projection FROM line_movements l6
                WHERE l6.prop_key = lm.prop_key
                ORDER BY l6.snapshot_at DESC LIMIT 1
            ) as projection
        FROM line_movements lm
        GROUP BY lm.prop_key
        ORDER BY last_updated DESC
        LIMIT ?1 OFFSET ?2
    "#;

    let rows = sqlx::query(query)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch line summaries: {}", e))?;

    let mut summaries = Vec::new();
    for row in &rows {
        let current_line: f64 = row.get("current_line");
        let opening_line: f64 = row.get("opening_line");
        let line_change = current_line - opening_line;

        let direction = if line_change > 0.05 {
            "up"
        } else if line_change < -0.05 {
            "down"
        } else {
            "stable"
        };

        // Apply direction filter
        if let Some(ref dir_filter) = filter.direction {
            if direction != dir_filter.as_str() {
                continue;
            }
        }

        // Apply min_change filter
        if let Some(min) = filter.min_change {
            if line_change.abs() < min {
                continue;
            }
        }

        summaries.push(LineMovementSummary {
            prop_key: row.get("prop_key"),
            player_name: row.get("player_name"),
            team: row.get("team"),
            opponent: row.get("opponent"),
            stat_category: row.get("stat_category"),
            league: row.get("league"),
            game_time: row.get("game_time"),
            current_line,
            opening_line,
            line_change,
            max_line: row.get("max_line"),
            min_line: row.get("min_line"),
            snapshot_count: row.get("snapshot_count"),
            first_seen: row.get("first_seen"),
            last_updated: row.get("last_updated"),
            direction: direction.to_string(),
            projection: row.get("projection"),
        });
    }

    Ok(LineMovementPage {
        summaries,
        total,
        limit,
        offset,
    })
}

/// Get detailed history for a specific prop
pub async fn get_line_detail(
    pool: &Pool<Sqlite>,
    prop_key: &str,
) -> Result<Option<LineDetailHistory>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, player_name, team, opponent, stat_category, league,
               line, projection, source, game_time, snapshot_at, prop_key
        FROM line_movements
        WHERE prop_key = ?1
        ORDER BY snapshot_at ASC
        "#,
    )
    .bind(prop_key)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch line detail: {}", e))?;

    if rows.is_empty() {
        return Ok(None);
    }

    let snapshots: Vec<LineMovementRecord> = rows
        .iter()
        .map(|r| LineMovementRecord {
            id: r.get("id"),
            player_name: r.get("player_name"),
            team: r.get("team"),
            opponent: r.get("opponent"),
            stat_category: r.get("stat_category"),
            league: r.get("league"),
            line: r.get("line"),
            projection: r.get("projection"),
            source: r.get("source"),
            game_time: r.get("game_time"),
            snapshot_at: r.get("snapshot_at"),
            prop_key: r.get("prop_key"),
        })
        .collect();

    let first = snapshots.first().unwrap();
    let last = snapshots.last().unwrap();
    let line_change = last.line - first.line;
    let max_line = snapshots.iter().map(|s| s.line).fold(f64::MIN, f64::max);
    let min_line = snapshots.iter().map(|s| s.line).fold(f64::MAX, f64::min);

    let direction = if line_change > 0.05 {
        "up"
    } else if line_change < -0.05 {
        "down"
    } else {
        "stable"
    };

    Ok(Some(LineDetailHistory {
        summary: LineMovementSummary {
            prop_key: prop_key.to_string(),
            player_name: first.player_name.clone(),
            team: first.team.clone(),
            opponent: first.opponent.clone(),
            stat_category: first.stat_category.clone(),
            league: first.league.clone(),
            game_time: first.game_time.clone(),
            current_line: last.line,
            opening_line: first.line,
            line_change,
            max_line,
            min_line,
            snapshot_count: snapshots.len() as i64,
            first_seen: first.snapshot_at.clone(),
            last_updated: last.snapshot_at.clone(),
            direction: direction.to_string(),
            projection: last.projection,
        },
        snapshots,
    }))
}

/// Get list of distinct leagues that have line data
pub async fn get_tracked_leagues(pool: &Pool<Sqlite>) -> Result<Vec<String>, String> {
    let rows = sqlx::query("SELECT DISTINCT league FROM line_movements ORDER BY league")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch leagues: {}", e))?;

    Ok(rows.iter().map(|r| r.get::<String, _>("league")).collect())
}

/// Get list of distinct stat categories that have line data
pub async fn get_tracked_stat_categories(pool: &Pool<Sqlite>) -> Result<Vec<String>, String> {
    let rows = sqlx::query("SELECT DISTINCT stat_category FROM line_movements ORDER BY stat_category")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to fetch stat categories: {}", e))?;

    Ok(rows.iter().map(|r| r.get::<String, _>("stat_category")).collect())
}

/// Delete old snapshots beyond a retention period (default 30 days)
pub async fn prune_old_snapshots(
    pool: &Pool<Sqlite>,
    retention_days: i64,
) -> Result<u64, String> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(retention_days)).to_rfc3339();
    let result = sqlx::query("DELETE FROM line_movements WHERE snapshot_at < ?1")
        .bind(&cutoff)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to prune old snapshots: {}", e))?;

    Ok(result.rows_affected())
}

/// Get the latest snapshot timestamp
pub async fn get_latest_snapshot_time(pool: &Pool<Sqlite>) -> Result<Option<String>, String> {
    let row: Option<String> = sqlx::query_scalar(
        "SELECT MAX(snapshot_at) FROM line_movements",
    )
    .fetch_one(pool)
    .await
    .ok();

    Ok(row)
}

// ═══════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════

/// Build a unique key for a prop to group line movements
pub fn build_prop_key(
    player_name: &str,
    stat_category: &str,
    league: &str,
    game_time: Option<&str>,
) -> String {
    format!(
        "{}|{}|{}|{}",
        player_name.to_lowercase(),
        stat_category.to_lowercase(),
        league.to_lowercase(),
        game_time.unwrap_or("unknown")
    )
}

/// Parse a prop_key back into its components
pub fn parse_prop_key(key: &str) -> (String, String, String, Option<String>) {
    let parts: Vec<&str> = key.splitn(4, '|').collect();
    (
        parts.get(0).unwrap_or(&"").to_string(),
        parts.get(1).unwrap_or(&"").to_string(),
        parts.get(2).unwrap_or(&"").to_string(),
        parts.get(3).filter(|s| **s != "unknown").map(|s| s.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_prop_key() {
        let key = build_prop_key("Patrick Mahomes", "Passing Yards", "NFL", Some("2025-01-15T20:00:00Z"));
        assert_eq!(key, "patrick mahomes|passing yards|nfl|2025-01-15T20:00:00Z");
    }

    #[test]
    fn test_parse_prop_key() {
        let (player, stat, league, game) = parse_prop_key("patrick mahomes|passing yards|nfl|2025-01-15T20:00:00Z");
        assert_eq!(player, "patrick mahomes");
        assert_eq!(stat, "passing yards");
        assert_eq!(league, "nfl");
        assert_eq!(game, Some("2025-01-15T20:00:00Z".to_string()));
    }

    #[test]
    fn test_parse_prop_key_no_game() {
        let (player, stat, league, game) = parse_prop_key("patrick mahomes|passing yards|nfl|unknown");
        assert_eq!(player, "patrick mahomes");
        assert_eq!(game, None);
    }
}
