#![allow(dead_code)]
//! ═══════════════════════════════════════════════════════════════
//! Game-Day Notification Engine
//!
//! Polls ESPN scoreboard for game state changes and emits
//! notifications when:
//!   - A game involving a predicted player is about to start
//!   - A game goes final (predictions can now be graded)
//!   - A prediction is auto-graded after game completion
//!
//! Uses tauri-plugin-notification for OS-level desktop alerts
//! and Tauri events for in-app notification center updates.
//! ═══════════════════════════════════════════════════════════════

use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite, Row};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;
use tauri::Emitter;

use crate::football::live_data::{fetch_live_scoreboard_for_league, SportLeague};
use crate::predictions::grading;
use crate::predictions::tracker::{PredictionOutcome, PredictionRecord, PredictionTracker};

// ═══════════════════════════════════════════════════════════════
// Data Types
// ═══════════════════════════════════════════════════════════════

/// A notification displayed to the user
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppNotification {
    pub id: String,
    pub notification_type: NotificationType,
    pub title: String,
    pub body: String,
    pub player_name: Option<String>,
    pub game_id: Option<String>,
    pub prediction_id: Option<String>,
    pub created_at: String,
    pub read: bool,
    pub dismissed: bool,
}

/// Types of notifications the system can generate
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum NotificationType {
    GameStarting,
    GameFinal,
    PredictionGraded,
    PredictionWin,
    PredictionLoss,
    PredictionPush,
    GradingComplete,
    /// A Kalshi market prediction was graded as a win
    KalshiMarketWin,
    /// A Kalshi market prediction was graded as a loss
    KalshiMarketLoss,
    Info,
}

impl std::fmt::Display for NotificationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotificationType::GameStarting => write!(f, "game_starting"),
            NotificationType::GameFinal => write!(f, "game_final"),
            NotificationType::PredictionGraded => write!(f, "prediction_graded"),
            NotificationType::PredictionWin => write!(f, "prediction_win"),
            NotificationType::PredictionLoss => write!(f, "prediction_loss"),
            NotificationType::PredictionPush => write!(f, "prediction_push"),
            NotificationType::GradingComplete => write!(f, "grading_complete"),
            NotificationType::KalshiMarketWin => write!(f, "kalshi_market_win"),
            NotificationType::KalshiMarketLoss => write!(f, "kalshi_market_loss"),
            NotificationType::Info => write!(f, "info"),
        }
    }
}

impl std::str::FromStr for NotificationType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "game_starting" => Ok(NotificationType::GameStarting),
            "game_final" => Ok(NotificationType::GameFinal),
            "prediction_graded" => Ok(NotificationType::PredictionGraded),
            "prediction_win" => Ok(NotificationType::PredictionWin),
            "prediction_loss" => Ok(NotificationType::PredictionLoss),
            "prediction_push" => Ok(NotificationType::PredictionPush),
            "grading_complete" => Ok(NotificationType::GradingComplete),
            "kalshi_market_win" => Ok(NotificationType::KalshiMarketWin),
            "kalshi_market_loss" => Ok(NotificationType::KalshiMarketLoss),
            "info" => Ok(NotificationType::Info),
            _ => Err(format!("Unknown notification type: {}", s)),
        }
    }
}

/// User preferences for notifications
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub game_starting_enabled: bool,
    pub game_final_enabled: bool,
    pub prediction_graded_enabled: bool,
    pub grading_complete_enabled: bool,
    #[serde(default = "default_kalshi_notifications_enabled")]
    pub kalshi_notifications_enabled: bool,
    pub poll_interval_secs: u64,
    pub game_starting_minutes_before: u32,
    pub show_os_notifications: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            game_starting_enabled: true,
            game_final_enabled: true,
            prediction_graded_enabled: true,
            grading_complete_enabled: true,
            kalshi_notifications_enabled: true,
            poll_interval_secs: 60,
            game_starting_minutes_before: 30,
            show_os_notifications: true,
        }
    }
}

const NOTIFICATION_SETTINGS_FILE: &str = "notification_settings.json";

fn default_kalshi_notifications_enabled() -> bool {
    true
}

/// Path to the persisted notification settings file (alongside `config.json`).
pub fn settings_path() -> std::path::PathBuf {
    crate::config::config_dir().join(NOTIFICATION_SETTINGS_FILE)
}

/// Load notification settings from disk, falling back to defaults if the file
/// is missing or unreadable.
pub fn load_settings() -> NotificationSettings {
    let path = settings_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(settings) = serde_json::from_str::<NotificationSettings>(&content) {
            return settings;
        }
    }
    NotificationSettings::default()
}

/// Persist notification settings to disk as pretty-printed JSON.
pub fn save_settings(settings: &NotificationSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config dir: {}", e))?;
    }
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize notification settings: {}", e))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("Failed to write notification settings: {}", e))?;
    Ok(())
}

/// Whether Kalshi market win/loss notifications should be emitted.
pub fn kalshi_market_notifications_enabled(settings: &NotificationSettings) -> bool {
    settings.enabled && settings.kalshi_notifications_enabled
}

/// Whether grading-complete summary notifications should be emitted.
pub fn grading_summary_notifications_enabled(settings: &NotificationSettings) -> bool {
    settings.enabled && settings.grading_complete_enabled
}

/// Tracked game state for detecting transitions
#[derive(Debug, Clone, PartialEq)]
pub enum GameStatus {
    Scheduled,
    InProgress,
    Final,
    Unknown(String),
}

impl GameStatus {
    fn from_espn(status: &str) -> Self {
        match status {
            "STATUS_SCHEDULED" | "STATUS_PRE_GAME" => GameStatus::Scheduled,
            "STATUS_IN_PROGRESS" | "STATUS_FIRST_HALF" | "STATUS_SECOND_HALF"
            | "STATUS_HALFTIME" | "STATUS_OVERTIME" | "STATUS_2ND_OVERTIME" => {
                GameStatus::InProgress
            }
            "STATUS_FINAL" | "STATUS_FULL_TIME" | "STATUS_FINAL_OVERTIME" => GameStatus::Final,
            other => GameStatus::Unknown(other.to_string()),
        }
    }
}

/// Snapshot of a tracked game
#[derive(Debug, Clone)]
struct TrackedGame {
    game_id: String,
    home_team: String,
    away_team: String,
    status: GameStatus,
    home_players: Vec<String>, // player names from pending predictions
    away_players: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════
// Database Operations
// ═══════════════════════════════════════════════════════════════

/// Ensure the notifications table exists
pub async fn init_notifications_table(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS notifications (
            id TEXT PRIMARY KEY,
            notification_type TEXT NOT NULL,
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            player_name TEXT,
            game_id TEXT,
            prediction_id TEXT,
            created_at TEXT NOT NULL,
            read INTEGER NOT NULL DEFAULT 0,
            dismissed INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create notifications table: {}", e))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_notif_created ON notifications(created_at)")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_notif_read ON notifications(read)")
        .execute(pool)
        .await
        .ok();

    Ok(())
}

/// Insert a notification into the database
pub async fn insert_notification(
    pool: &Pool<Sqlite>,
    notification: &AppNotification,
) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO notifications
            (id, notification_type, title, body, player_name, game_id,
             prediction_id, created_at, read, dismissed)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#,
    )
    .bind(&notification.id)
    .bind(notification.notification_type.to_string())
    .bind(&notification.title)
    .bind(&notification.body)
    .bind(&notification.player_name)
    .bind(&notification.game_id)
    .bind(&notification.prediction_id)
    .bind(&notification.created_at)
    .bind(if notification.read { 1 } else { 0 })
    .bind(if notification.dismissed { 1 } else { 0 })
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to insert notification: {}", e))?;

    Ok(())
}

/// Get all notifications, newest first
pub async fn get_notifications(
    pool: &Pool<Sqlite>,
    limit: Option<i64>,
) -> Result<Vec<AppNotification>, String> {
    let limit = limit.unwrap_or(100);
    let rows = sqlx::query(
        r#"
        SELECT id, notification_type, title, body, player_name, game_id,
               prediction_id, created_at, read, dismissed
        FROM notifications
        ORDER BY created_at DESC
        LIMIT ?1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch notifications: {}", e))?;

    Ok(rows.iter().map(row_to_notification).collect())
}

/// Get unread notification count
pub async fn get_unread_count(pool: &Pool<Sqlite>) -> Result<i64, String> {
    let row = sqlx::query(
        "SELECT COUNT(*) as cnt FROM notifications WHERE read = 0 AND dismissed = 0",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("Failed to count unread: {}", e))?;

    Ok(row.get::<i64, _>("cnt"))
}

/// Mark a notification as read
pub async fn mark_read(pool: &Pool<Sqlite>, id: &str) -> Result<(), String> {
    sqlx::query("UPDATE notifications SET read = 1 WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to mark read: {}", e))?;
    Ok(())
}

/// Mark all notifications as read
pub async fn mark_all_read(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query("UPDATE notifications SET read = 1 WHERE read = 0")
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to mark all read: {}", e))?;
    Ok(())
}

/// Dismiss a notification
pub async fn dismiss_notification(pool: &Pool<Sqlite>, id: &str) -> Result<(), String> {
    sqlx::query("UPDATE notifications SET dismissed = 1 WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to dismiss: {}", e))?;
    Ok(())
}

/// Delete old notifications (older than N days)
pub async fn cleanup_old_notifications(
    pool: &Pool<Sqlite>,
    days: i64,
) -> Result<u64, String> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
    let result = sqlx::query("DELETE FROM notifications WHERE created_at < ?1")
        .bind(&cutoff)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to cleanup notifications: {}", e))?;

    Ok(result.rows_affected())
}

fn row_to_notification(r: &sqlx::sqlite::SqliteRow) -> AppNotification {
    let notif_type_str: String = r.get("notification_type");
    let read_int: i64 = r.get("read");
    let dismissed_int: i64 = r.get("dismissed");

    AppNotification {
        id: r.get("id"),
        notification_type: notif_type_str
            .parse::<NotificationType>()
            .unwrap_or(NotificationType::Info),
        title: r.get("title"),
        body: r.get("body"),
        player_name: r.get("player_name"),
        game_id: r.get("game_id"),
        prediction_id: r.get("prediction_id"),
        created_at: r.get("created_at"),
        read: read_int != 0,
        dismissed: dismissed_int != 0,
    }
}

// ═══════════════════════════════════════════════════════════════
// Notification Engine — Polling + Game State Tracking
// ═══════════════════════════════════════════════════════════════

/// The notification engine state shared across the app
pub struct NotificationEngine {
    pool: Pool<Sqlite>,
    tracker: Arc<Mutex<PredictionTracker>>,
    settings: NotificationSettings,
    /// Tracks game states from previous poll cycle to detect transitions
    game_states: HashMap<String, GameStatus>,
}

impl NotificationEngine {
    pub fn new(
        pool: Pool<Sqlite>,
        tracker: Arc<Mutex<PredictionTracker>>,
        settings: NotificationSettings,
    ) -> Self {
        Self {
            pool,
            tracker,
            settings,
            game_states: HashMap::new(),
        }
    }

    pub fn update_settings(&mut self, settings: NotificationSettings) {
        self.settings = settings;
    }

    /// Run a single poll cycle. Called periodically from the background task.
    pub async fn poll_cycle(
        &mut self,
        app_handle: &tauri::AppHandle,
    ) -> Result<(), String> {
        if !self.settings.enabled {
            return Ok(());
        }

        // Fetch current scoreboard
        let games = match fetch_live_scoreboard_for_league(SportLeague::NFL).await {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("Notification poll: failed to fetch scoreboard: {}", e);
                return Ok(()); // Don't fail the whole cycle on a fetch error
            }
        };

        // Get pending predictions with player names
        let pending_predictions = {
            let t = self.tracker.lock().await;
            let all = t.get_all_predictions().await;
            all.into_iter()
                .filter(|r| r.outcome == PredictionOutcome::Pending)
                .filter(|r| {
                    r.prediction.player_name.is_some()
                        && r.prediction.stat_category.is_some()
                        && r.prediction.line.is_some()
                        && r.prediction.pick_type.is_some()
                })
                .collect::<Vec<_>>()
        };

        // Build a map of player_name -> predictions for quick lookup
        let mut player_predictions: HashMap<String, Vec<&PredictionRecord>> = HashMap::new();
        for pred in &pending_predictions {
            if let Some(ref name) = pred.prediction.player_name {
                player_predictions
                    .entry(name.clone())
                    .or_default()
                    .push(pred);
            }
        }

        // Check each game for state transitions
        for game in &games {
            let game_id = format!("{}_{}", game.home_team, game.away_team);
            let current_status = GameStatus::from_espn(&game.game_time);
            let previous_status = self.game_states.get(&game_id).cloned();

            // Detect game starting (Scheduled -> InProgress)
            if self.settings.game_starting_enabled
                && previous_status == Some(GameStatus::Scheduled)
                && current_status == GameStatus::InProgress
            {
                self.handle_game_starting(
                    app_handle,
                    game,
                    &player_predictions,
                )
                .await;
            }

            // Detect game final (InProgress -> Final)
            if self.settings.game_final_enabled
                && previous_status == Some(GameStatus::InProgress)
                && current_status == GameStatus::Final
            {
                self.handle_game_final(app_handle, game, &player_predictions)
                    .await;
            }

            // Update tracked state
            self.game_states.insert(game_id, current_status);
        }

        // Also check for games that disappeared from scoreboard (ended)
        // by comparing with previously tracked games
        let current_game_keys: std::collections::HashSet<String> = games
            .iter()
            .map(|g| format!("{}_{}", g.home_team, g.away_team))
            .collect();

        self.game_states
            .retain(|k, _| current_game_keys.contains(k));

        Ok(())
    }

    async fn handle_game_starting(
        &self,
        app_handle: &tauri::AppHandle,
        game: &crate::football::data::GameInfo,
        player_predictions: &HashMap<String, Vec<&PredictionRecord>>,
    ) {
        // Find predictions involving players in this game
        // We check both home and away team abbreviations against player names
        let game_label = format!("{} @ {}", game.away_team, game.home_team);

        for (player_name, preds) in player_predictions {
            // Simple heuristic: if we have pending predictions for a player,
            // and a game is starting, notify. In a more sophisticated version
            // we'd cross-reference team rosters.
            for _pred in preds {
                let notif = AppNotification {
                    id: uuid::Uuid::new_v4().to_string(),
                    notification_type: NotificationType::GameStarting,
                    title: format!("🏈 Game Starting: {}", game_label),
                    body: format!(
                        "{}'s game is about to kick off. Your pending prediction is in play!",
                        player_name
                    ),
                    player_name: Some(player_name.clone()),
                    game_id: Some(format!("{}_{}", game.home_team, game.away_team)),
                    prediction_id: None,
                    created_at: chrono::Utc::now().to_rfc3339(),
                    read: false,
                    dismissed: false,
                };

                self.emit_notification(app_handle, notif).await;
            }
        }
    }

    async fn handle_game_final(
        &self,
        app_handle: &tauri::AppHandle,
        game: &crate::football::data::GameInfo,
        player_predictions: &HashMap<String, Vec<&PredictionRecord>>,
    ) {
        let game_label = format!("{} @ {}", game.away_team, game.home_team);

        // Emit game final notification
        let notif = AppNotification {
            id: uuid::Uuid::new_v4().to_string(),
            notification_type: NotificationType::GameFinal,
            title: format!("🏁 Game Final: {}", game_label),
            body: format!(
                "{} has ended. Your predictions can now be graded.",
                game_label
            ),
            player_name: None,
            game_id: Some(format!("{}_{}", game.home_team, game.away_team)),
            prediction_id: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            read: false,
            dismissed: false,
        };

        self.emit_notification(app_handle, notif).await;

        // Auto-grade predictions for players in this game
        if self.settings.prediction_graded_enabled {
            self.auto_grade_for_game(app_handle, game, player_predictions)
                .await;
        }
    }

    async fn auto_grade_for_game(
        &self,
        app_handle: &tauri::AppHandle,
        game: &crate::football::data::GameInfo,
        player_predictions: &HashMap<String, Vec<&PredictionRecord>>,
    ) {
        // Collect pending predictions that could be graded
        let pending: Vec<_> = player_predictions
            .iter()
            .flat_map(|(_, preds)| preds.iter().map(|p| {
                (
                    p.prediction.id.clone(),
                    p.prediction.player_name.clone().unwrap_or_default(),
                    p.prediction.pick_type.clone().unwrap_or_default(),
                    p.prediction.line.unwrap_or(0.0),
                    p.prediction.stat_category.clone().unwrap_or_default(),
                    p.prediction.session_id.clone(),
                )
            }))
            .collect();

        if pending.is_empty() {
            return;
        }

        // Run grading
        let summary = grading::grade_all_pending(&pending).await;

        // Apply results and emit notifications
        let t = self.tracker.lock().await;
        for result in &summary.results {
            match result.outcome.as_str() {
                "Win" | "Loss" | "Push" => {
                    let outcome = result
                        .outcome
                        .parse::<PredictionOutcome>()
                        .unwrap_or(PredictionOutcome::Pending);
                    let _ = t
                        .update_outcome(&result.prediction_id, outcome, result.actual_result)
                        .await;

                    // Emit per-prediction notification
                    let (notif_type, emoji) = match result.outcome.as_str() {
                        "Win" => (NotificationType::PredictionWin, "✅"),
                        "Loss" => (NotificationType::PredictionLoss, "❌"),
                        "Push" => (NotificationType::PredictionPush, "🔄"),
                        _ => (NotificationType::PredictionGraded, "📊"),
                    };

                    let notif = AppNotification {
                        id: uuid::Uuid::new_v4().to_string(),
                        notification_type: notif_type,
                        title: format!(
                            "{} {}: {}",
                            emoji, result.outcome, result.player_name
                        ),
                        body: format!(
                            "{} — {} {} (Line: {}). Actual: {}",
                            result.player_name,
                            result.pick_type,
                            result.line,
                            result.stat_category,
                            result.actual_result.unwrap_or(0.0)
                        ),
                        player_name: Some(result.player_name.clone()),
                        game_id: Some(format!("{}_{}", game.home_team, game.away_team)),
                        prediction_id: Some(result.prediction_id.clone()),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        read: false,
                        dismissed: false,
                    };

                    self.emit_notification(app_handle, notif).await;
                }
                _ => {}
            }
        }

        // Emit grading complete summary
        if self.settings.grading_complete_enabled {
            let notif = AppNotification {
                id: uuid::Uuid::new_v4().to_string(),
                notification_type: NotificationType::GradingComplete,
                title: format!(
                    "📊 Grading Complete: {} graded, {} unresolved",
                    summary.graded, summary.unresolved
                ),
                body: format!(
                    "W: {} | L: {} | P: {} from game {} @ {}",
                    summary.wins, summary.losses, summary.pushes,
                    game.away_team, game.home_team
                ),
                player_name: None,
                game_id: Some(format!("{}_{}", game.home_team, game.away_team)),
                prediction_id: None,
                created_at: chrono::Utc::now().to_rfc3339(),
                read: false,
                dismissed: false,
            };

            self.emit_notification(app_handle, notif).await;
        }
    }

    /// Save notification to DB, emit Tauri event, and optionally show OS notification
    async fn emit_notification(
        &self,
        app_handle: &tauri::AppHandle,
        notif: AppNotification,
    ) {
        // Persist to DB
        if let Err(e) = insert_notification(&self.pool, &notif).await {
            tracing::warn!("Failed to persist notification: {}", e);
        }

        // Emit in-app event
        let _ = app_handle.emit("notification-new", &notif);

        // Show OS notification if enabled
        if self.settings.show_os_notifications {
            use tauri_plugin_notification::NotificationExt;
            let _ = app_handle
                .notification()
                .builder()
                .title(&notif.title)
                .body(&notif.body)
                .show();
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Background Polling Task
// ═══════════════════════════════════════════════════════════════

/// Spawn the notification polling background task.
/// This runs indefinitely until the app exits.
pub fn spawn_polling_task(
    app_handle: tauri::AppHandle,
    pool: Pool<Sqlite>,
    tracker: Arc<Mutex<PredictionTracker>>,
    settings: NotificationSettings,
) {
    let mut engine = NotificationEngine::new(pool, tracker, settings.clone());
    let poll_interval = settings.poll_interval_secs.max(15); // Minimum 15s

    tauri::async_runtime::spawn(async move {
        let mut ticker = interval(Duration::from_secs(poll_interval));

        // Initialize game states on first run
        if let Ok(games) = fetch_live_scoreboard_for_league(SportLeague::NFL).await {
            for game in &games {
                let game_id = format!("{}_{}", game.home_team, game.away_team);
                let status = GameStatus::from_espn(&game.game_time);
                engine.game_states.insert(game_id, status);
            }
            tracing::info!(
                "Notification engine initialized with {} tracked games",
                engine.game_states.len()
            );
        }

        loop {
            ticker.tick().await;

            if let Err(e) = engine.poll_cycle(&app_handle).await {
                tracing::warn!("Notification poll cycle error: {}", e);
            }
        }
    });
}

#[cfg(test)]
mod notification_settings_tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn notification_type_kalshi_roundtrip() {
        assert_eq!(
            NotificationType::KalshiMarketWin.to_string(),
            "kalshi_market_win"
        );
        assert_eq!(
            NotificationType::from_str("kalshi_market_loss").unwrap(),
            NotificationType::KalshiMarketLoss
        );
    }

    #[test]
    fn settings_missing_kalshi_field_defaults_true() {
        let json = r#"{
            "enabled": true,
            "game_starting_enabled": true,
            "game_final_enabled": true,
            "prediction_graded_enabled": true,
            "grading_complete_enabled": true,
            "poll_interval_secs": 60,
            "game_starting_minutes_before": 30,
            "show_os_notifications": true
        }"#;
        let s: NotificationSettings = serde_json::from_str(json).unwrap();
        assert!(s.kalshi_notifications_enabled);
    }

    #[test]
    fn kalshi_market_gated_by_master_switch() {
        let mut s = NotificationSettings::default();
        assert!(kalshi_market_notifications_enabled(&s));
        s.enabled = false;
        assert!(!kalshi_market_notifications_enabled(&s));
        s.enabled = true;
        s.kalshi_notifications_enabled = false;
        assert!(!kalshi_market_notifications_enabled(&s));
    }
}
