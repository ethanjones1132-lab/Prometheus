pub mod analysis;
pub mod correlation;
pub mod bot;
pub mod commands;
pub mod config;
pub mod error;
pub mod eval_adapter;
pub mod football;
pub mod chat;
pub mod predictions;
pub mod weather;
pub mod bankroll;
pub mod notification;
pub mod kalshi;
pub mod line_tracker;
pub mod ml_predictor;
pub mod prizepicks;
pub mod paper;

use chat::ChatState;
use football::api_client::{SportsApiClient, SportsApiConfig};
use predictions::tracker::PredictionTracker;
use prizepicks::PrizePicksFetcher;

use std::sync::Arc;
use tokio::sync::Mutex;
use weather::WeatherClient;
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    rt.block_on(async {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_file(true)
            .with_line_number(true)
            .init();
    });

    // Initialize SQLite database
    let db_pool = rt.block_on(async {
        predictions::storage::init_db()
            .await
            .expect("Failed to initialize database")
    });

    // Initialize line movement tracking tables
    rt.block_on(async {
        if let Err(e) = line_tracker::init_line_tables(&db_pool).await {
            tracing::warn!("Failed to init line tables: {}", e);
        }
    });

    // Initialize Kalshi price snapshot tables
    rt.block_on(async {
        if let Err(e) = kalshi::price_tracker::init_price_tables(&db_pool).await {
            tracing::warn!("Failed to init kalshi price tables: {}", e);
        }
    });

    // Initialize ML prediction tables
    rt.block_on(async {
        if let Err(e) = ml_predictor::init_ml_tables(&db_pool).await {
            tracing::warn!("Failed to init ML tables: {}", e);
        }
    });

    // Initialize paper-trading journal tables
    rt.block_on(async {
        if let Err(e) = paper::init_paper_tables(&db_pool).await {
            tracing::warn!("Failed to init paper tables: {}", e);
        }
    });

    // Initialize prediction tracker (migrates JSON data on first run)
    let prediction_tracker = rt.block_on(async {
        PredictionTracker::new(db_pool.clone())
            .await
    });

    let app_config = Arc::new(Mutex::new(config::load_config()));
    let chat_state = Arc::new(Mutex::new(ChatState::default()));
    let prediction_tracker = Arc::new(Mutex::new(prediction_tracker));

    let weather_client = Arc::new(Mutex::new(WeatherClient::new(
        config::load_config().openweathermap_api_key,
    )));
    let kalshi_client = Arc::new(Mutex::new(kalshi::KalshiClient::new(
        kalshi::kalshi_config_from_app(&config::load_config()),
    )));
    let api_client = Arc::new(Mutex::new(
        SportsApiClient::new(SportsApiConfig::default())
            .expect("Failed to create sports API client"),
    ));
    let prizepicks_fetcher = Arc::new(Mutex::new(PrizePicksFetcher));
    let db_pool_state = db_pool.clone();
    let prediction_tracker_for_setup = prediction_tracker.clone();
    let kalshi_for_grade = kalshi_client.clone();
    let kalshi_for_warm = kalshi_client.clone();
    let kalshi_auto_grade_secs = config::load_config().kalshi_poll_interval_secs.max(60);

    tauri::Builder::default()
        .setup(move |app| {
            // Initialize notifications table
            let notif_pool = db_pool.clone();
            rt.block_on(async {
                if let Err(e) = notification::init_notifications_table(&notif_pool).await {
                    tracing::warn!("Failed to init notifications table: {}", e);
                }
            });

            // Spawn notification polling background task
            let notif_settings = notification::NotificationSettings::default();
            notification::spawn_polling_task(
                app.handle().clone(),
                db_pool.clone(),
                prediction_tracker_for_setup.clone(),
                notif_settings,
            );

            // Background auto-grade for resolved Kalshi markets
            kalshi::spawn_auto_grade_task(
                kalshi_for_grade.clone(),
                prediction_tracker_for_setup.clone(),
                kalshi_auto_grade_secs,
            );

            // Settle open paper lots when Kalshi markets resolve
            paper::spawn_paper_settle_task(
                db_pool.clone(),
                kalshi_for_grade,
                kalshi_auto_grade_secs,
            );

            // Warm full Kalshi catalog in the background (dashboard uses quick cache first)
            let kalshi_warm = kalshi_for_warm.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                let mut client = kalshi_warm.lock().await;
                if client.needs_full_catalog() {
                    if let Err(e) = client.fetch_all_markets().await {
                        tracing::warn!("kalshi background cache warm failed: {}", e);
                    } else {
                        tracing::info!("kalshi full catalog cache warmed");
                    }
                }
            });

            Ok(())
        })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .manage(app_config)
        .manage(chat_state)
        .manage(prediction_tracker)

        .manage(weather_client)
        .manage(api_client)
        .manage(kalshi_client)
        .manage(prizepicks_fetcher)
        .manage(db_pool_state)
        .invoke_handler(tauri::generate_handler![
            // Config
            commands::get_config,
            commands::save_config,
            commands::check_api_status,
            commands::get_security_posture,
            commands::get_available_models,
            // Chat
            commands::send_message,
            commands::send_message_stream,
            commands::new_chat_session,
            commands::list_chat_sessions,
            commands::delete_chat_session,
            commands::get_session_messages,
            // Football data
            commands::search_players,
            commands::get_game_schedule,
            commands::fetch_live_scoreboard,
            commands::fetch_nfl_standings,
            commands::fetch_nfl_news,
            commands::fetch_sleeper_state,
            commands::fetch_sleeper_injuries,
            commands::fetch_sleeper_stats,
            commands::fetch_live_data_context,
            commands::get_data_source_status,

            // Weather
            commands::get_game_weather,
            // Predictions
            commands::get_session_predictions,
            commands::get_all_predictions,
            commands::get_prediction_stats,
            commands::update_prediction_outcome,
            commands::get_predictions_by_confidence,
            commands::get_overall_trend,
            commands::get_player_trend,
            commands::get_stat_category_trend,
            commands::get_trend_player_list,
            commands::get_trend_stat_category_list,
            commands::get_parlay_legs,
            commands::compare_models,
            // Grading
            commands::grade_pending_predictions,
            commands::get_grading_status,
            commands::export_predictions_csv,
            // Bankroll Management
            commands::get_bankroll_config,
            commands::save_bankroll_config,
            commands::get_bankroll_summary,
            commands::recommend_bets,
            commands::recommend_parlay,
            commands::record_bankroll_result,
            commands::refresh_historical_brier,
            // Multi-Sport Scoreboard
            commands::fetch_league_scoreboard,
            commands::fetch_all_scoreboards,
            commands::get_sport_league_data,
            commands::inject_sports_data,
            // Live Player Stats (multi-sport API)
            commands::fetch_season_leaders,
            commands::fetch_player_stats_by_id,
            commands::fetch_team_players,
            commands::fetch_season_leaders_map,
            commands::fetch_multi_sport_leaders,
            commands::build_live_player_context,
            // Notifications
            commands::get_notifications,
            commands::get_unread_notification_count,
            commands::mark_notification_read,
            commands::mark_all_notifications_read,
            commands::dismiss_notification_cmd,
            commands::get_notification_settings,
            commands::save_notification_settings,
            // File upload
            commands::read_file_base64,
            // Kalshi
            commands::kalshi_get_markets,
            commands::kalshi_get_market,
            commands::kalshi_get_orderbook,
            commands::kalshi_search_markets,
            commands::kalshi_get_top_markets,
            commands::kalshi_get_dashboard_bootstrap,
            commands::kalshi_get_category_stats,
            commands::kalshi_get_portfolio,
            commands::kalshi_refresh,
            commands::kalshi_get_predictions,
            commands::kalshi_get_prediction_stats,
            commands::kalshi_grade_pending_predictions,
            commands::kalshi_get_grading_summary,
            commands::export_kalshi_predictions_csv,
            commands::kalshi_compute_stake_adjustment,
            commands::kalshi_get_calibration_status,
            commands::kalshi_snapshot_prices,
            commands::kalshi_get_price_history,
            commands::kalshi_record_paper_decision,
            commands::paper_get_analytics,
            commands::paper_get_positions,
            commands::paper_settle_pending,
            commands::paper_reset_account,
            // Bot integration
            commands::get_bot_config,
            commands::save_bot_config,
            commands::test_discord_webhook_cmd,
            commands::test_telegram_bot_cmd,
            commands::send_bot_test_message,

            // Line Movement Tracking
            commands::snapshot_line_movements,
            commands::get_line_movements,
            commands::get_line_detail,
            commands::get_tracked_line_leagues,
            commands::get_tracked_line_stat_categories,
            commands::get_latest_line_snapshot,
            commands::prune_line_movements,
            // Analysis engine
            commands::analyze_prop,
            commands::analyze_multiple_props,
            commands::get_scored_props_by_tier,
            commands::analyze_parlay_correlation,
            commands::generate_analysis_context,
            // ML Predictor
            commands::ml_train_model,
            commands::ml_predict_batch,
            commands::ml_get_model_status,
            commands::ml_get_predictions,
            commands::ml_export_features,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
