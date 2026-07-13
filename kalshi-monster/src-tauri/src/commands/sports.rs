#![allow(unused_imports)]

use crate::commands::{KalshiState, SharedCacheState, PickInput, KalshiDashboardBootstrap, edge_config_for_pool, emit_chat_kalshi_context};
use crate::chat::openrouter::{self, OpenRouterResponse};
use crate::chat::session;
use crate::config;
use crate::config::AppConfig;
use crate::error::AppError;
use crate::secrets::{AppSecrets, SecretKey};
use crate::football::data;
use crate::football::live_data;
use crate::football::player_stats;
use crate::predictions::tracker::{PredictionOutcome, PredictionRecord, PredictionTracker};
use crate::predictions::grading::{self, GradingSummary};
use crate::weather::WeatherClient;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::{Mutex, mpsc};

// ── Football Data Commands ──

#[tauri::command]
pub async fn search_players(query: String) -> Result<Vec<data::PlayerSearchResult>, String> {
    Ok(data::search_players(&query))
}

#[tauri::command]
pub async fn get_game_schedule(
    week: Option<u32>,
) -> Result<Vec<data::GameInfo>, String> {
    match live_data::fetch_live_schedule().await {
        Ok(schedule) if !schedule.is_empty() => Ok(schedule),
        Ok(_) => Ok(data::get_game_schedule(week)),
        Err(_) => Ok(data::get_game_schedule(week)),
    }
}

// ── Weather Commands ──

#[tauri::command]
pub async fn get_game_weather(
    game: String,
    location: String,
    weather: State<'_, Arc<Mutex<WeatherClient>>>,
) -> Result<crate::weather::GameWeather, String> {
    let mut w = weather.lock().await;
    w.get_weather(&game, &location).await
}

// ── Live Data / Sports API Commands ──

/// Get live data source status (which APIs are available)
#[tauri::command]
pub async fn get_data_source_status(
    state: State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<Vec<crate::football::api_client::DataSourceStatus>, String> {
    let config = state.lock().await.clone();
    let api_config = crate::football::api_client::SportsApiConfig {
        api_sports_key: config.api_sports_key.clone(),
        ..Default::default()
    };
    Ok(crate::football::api_client::check_all_sources(&api_config).await)
}

/// Fetch live NFL scoreboard from ESPN
#[tauri::command]
pub async fn fetch_live_scoreboard(
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.espn_scoreboard().await
}

/// Fetch NFL standings from ESPN
#[tauri::command]
pub async fn fetch_nfl_standings(
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.espn_standings().await
}

/// Fetch NFL news from ESPN
#[tauri::command]
pub async fn fetch_nfl_news(
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.espn_news().await
}

/// Fetch Sleeper NFL state (week, season, etc.)
#[tauri::command]
pub async fn fetch_sleeper_state(
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.sleeper_news().await
}

/// Fetch Sleeper injuries
#[tauri::command]
pub async fn fetch_sleeper_injuries(
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.sleeper_injuries().await
}

/// Fetch Sleeper player stats for a season/week
#[tauri::command]
pub async fn fetch_sleeper_stats(
    season: String,
    week: Option<u32>,
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;
    client.sleeper_player_stats(&season, week).await
}

/// Fetch comprehensive live data context for AI enrichment.
/// Returns a JSON object with schedule, standings, news, and injuries.
#[tauri::command]
pub async fn fetch_live_data_context(
    query: String,
) -> Result<serde_json::Value, String> {
    let client = crate::football::api_client::SportsApiClient::new(
        crate::football::api_client::SportsApiConfig::default(),
    )?;

    let mut result = serde_json::json!({});

    // Fetch scoreboard
    match client.espn_scoreboard().await {
        Ok(scoreboard) => {
            result["scoreboard"] = scoreboard;
        }
        Err(e) => {
            result["scoreboard_error"] = serde_json::json!(e);
        }
    }

    // Fetch standings
    match client.espn_standings().await {
        Ok(standings) => {
            result["standings"] = standings;
        }
        Err(e) => {
            result["standings_error"] = serde_json::json!(e);
        }
    }

    // Fetch news
    match client.espn_news().await {
        Ok(news) => {
            result["news"] = news;
        }
        Err(e) => {
            result["news_error"] = serde_json::json!(e);
        }
    }

    // Fetch Sleeper injuries
    match client.sleeper_injuries().await {
        Ok(injuries) => {
            result["injuries"] = injuries;
        }
        Err(e) => {
            result["injuries_error"] = serde_json::json!(e);
        }
    }

    // Fetch Sleeper state
    match client.sleeper_news().await {
        Ok(state) => {
            result["nfl_state"] = state;
        }
        Err(e) => {
            result["nfl_state_error"] = serde_json::json!(e);
        }
    }

    result["query"] = serde_json::json!(query);
    result["fetched_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());

    Ok(result)
}

// ── Multi-Sport Scoreboard Commands ──

/// Fetch the live scoreboard for a specific league from ESPN.
/// Supported leagues: "football" (NFL), "basketball" (NBA), "baseball" (MLB), "hockey" (NHL)
#[tauri::command]
pub async fn fetch_league_scoreboard(
    league: String,
) -> Result<serde_json::Value, String> {
    let (sport, league_code) = match league.to_lowercase().as_str() {
        "nfl" | "football" => ("football", "nfl"),
        "nba" | "basketball" => ("basketball", "nba"),
        "mlb" | "baseball" => ("baseball", "mlb"),
        "nhl" | "hockey" => ("hockey", "nhl"),
        _ => return Err(AppError::Validation(format!("Unsupported league: {}. Use NFL, NBA, MLB, or NHL.", league)).into()),
    };

    let base_url = format!(
        "https://site.api.espn.com/apis/site/v2/sports/{}/{}",
        sport, league_code
    );

    let url = format!("{}/scoreboard", base_url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Network(format!("HTTP client error: {}", e)))?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Api(format!("ESPN scoreboard request failed for {}: {}", league, e)))?;

    if !resp.status().is_success() {
        return Err(AppError::Api(format!(
            "ESPN scoreboard returned HTTP {} for {}",
            resp.status(),
            league
        )).into());
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Api(format!("ESPN scoreboard parse error for {}: {}", league, e)))?;

    Ok(serde_json::json!({
        "league": league.to_uppercase(),
        "sport": sport,
        "scoreboard": json,
        "fetched_at": chrono::Utc::now().to_rfc3339(),
    }))
}

/// Fetch scoreboards for all major leagues (NFL, NBA, MLB, NHL) in parallel.
/// Returns a JSON object keyed by league.
#[tauri::command]
pub async fn fetch_all_scoreboards() -> Result<serde_json::Value, String> {
    use futures::future::join_all;

    let leagues = vec!["football", "basketball", "baseball", "hockey"];
    let league_labels = vec!["NFL", "NBA", "MLB", "NHL"];

    let futures = leagues.iter().map(|&league| {
        let (sport, league_code) = match league {
            "football" => ("football", "nfl"),
            "basketball" => ("basketball", "nba"),
            "baseball" => ("baseball", "mlb"),
            "hockey" => ("hockey", "nhl"),
            _ => unreachable!(),
        };
        let url = format!(
            "https://site.api.espn.com/apis/site/v2/sports/{}/{}/scoreboard",
            sport, league_code
        );
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        async move {
            let result = client.get(&url).send().await;
            (league, result)
        }
    });

    let results = join_all(futures).await;
    let mut scoreboards = serde_json::json!({});

    for (i, (_league, result)) in results.into_iter().enumerate() {
        let label = league_labels[i];
        match result {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    scoreboards[label] = json;
                } else {
                    scoreboards[label] = serde_json::json!({ "error": "parse failed" });
                }
            }
            Ok(resp) => {
                scoreboards[label] = serde_json::json!({ "error": format!("HTTP {}", resp.status()) });
            }
            Err(e) => {
                scoreboards[label] = serde_json::json!({ "error": e.to_string() });
            }
        }
    }

    scoreboards["fetched_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
    Ok(scoreboards)
}

/// Get comprehensive data for a sport/league: scoreboard + standings + news.
/// Returns a JSON object with all available data for the league.
#[tauri::command]
pub async fn get_sport_league_data(
    league: String,
) -> Result<serde_json::Value, String> {
    let (sport, league_code) = match league.to_lowercase().as_str() {
        "nfl" | "football" => ("football", "nfl"),
        "nba" | "basketball" => ("basketball", "nba"),
        "mlb" | "baseball" => ("baseball", "mlb"),
        "nhl" | "hockey" => ("hockey", "nhl"),
        _ => return Err(AppError::Validation(format!("Unsupported league: {}. Use NFL, NBA, MLB, or NHL.", league)).into()),
    };

    let base_url = format!(
        "https://site.api.espn.com/apis/site/v2/sports/{}/{}",
        sport, league_code
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Network(format!("HTTP client error: {}", e)))?;

    let mut result = serde_json::json!({
        "league": league.to_uppercase(),
        "sport": sport,
    });

    // Fetch scoreboard
    let scoreboard_url = format!("{}/scoreboard", base_url);
    match client.get(&scoreboard_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                result["scoreboard"] = json;
            }
        }
        Ok(resp) => {
            result["scoreboard_error"] = serde_json::json!(format!("HTTP {}", resp.status()));
        }
        Err(e) => {
            result["scoreboard_error"] = serde_json::json!(e.to_string());
        }
    }

    // Fetch standings
    let standings_url = format!("{}/standings", base_url);
    match client.get(&standings_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                result["standings"] = json;
            }
        }
        Ok(resp) => {
            result["standings_error"] = serde_json::json!(format!("HTTP {}", resp.status()));
        }
        Err(e) => {
            result["standings_error"] = serde_json::json!(e.to_string());
        }
    }

    // Fetch news
    let news_url = format!("{}/news", base_url);
    match client.get(&news_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                result["news"] = json;
            }
        }
        Ok(resp) => {
            result["news_error"] = serde_json::json!(format!("HTTP {}", resp.status()));
        }
        Err(e) => {
            result["news_error"] = serde_json::json!(e.to_string());
        }
    }

    result["fetched_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
    Ok(result)
}

/// Key Call: Inject, store, and interpret multi-sport data dynamically
#[tauri::command]
pub async fn inject_sports_data(
    league: String,
    payload: serde_json::Value,
) -> Result<(), String> {
    data::inject_sports_data(&league, payload)
}

// ═══════════════════════════════════════════════════════════════
// Live Player Stats Commands — Multi-Sport API Integration
// ═══════════════════════════════════════════════════════════════

/// Fetch live season leaders for a sport league.
/// Returns top players across all major stat categories.
#[tauri::command]
pub async fn fetch_season_leaders(
    league: String,
    season: Option<u32>,
) -> Result<Vec<player_stats::PlayerStatProfile>, String> {
    let league = league.parse::<live_data::SportLeague>()
        .map_err(|e| AppError::Validation(format!("Invalid league: {}", e)))?;
    let fetcher = match season {
        Some(s) => player_stats::PlayerStatsFetcher::new(league)?.with_season(s),
        None => player_stats::PlayerStatsFetcher::new(league)?,
    };
    fetcher.fetch_season_leaders(league).await
}

/// Fetch live player stats by athlete ID.
#[tauri::command]
pub async fn fetch_player_stats_by_id(
    league: String,
    athlete_id: String,
    season: Option<u32>,
) -> Result<player_stats::PlayerStatProfile, String> {
    let league = league.parse::<live_data::SportLeague>()
        .map_err(|e| AppError::Validation(format!("Invalid league: {}", e)))?;
    let fetcher = match season {
        Some(s) => player_stats::PlayerStatsFetcher::new(league)?.with_season(s),
        None => player_stats::PlayerStatsFetcher::new(league)?,
    };
    fetcher.fetch_player_stats(league, &athlete_id).await
}

/// Fetch all players for a team (roster).
#[tauri::command]
pub async fn fetch_team_players(
    league: String,
    team_id: String,
    season: Option<u32>,
) -> Result<player_stats::TeamPlayerStats, String> {
    let league = league.parse::<live_data::SportLeague>()
        .map_err(|e| AppError::Validation(format!("Invalid league: {}", e)))?;
    let fetcher = match season {
        Some(s) => player_stats::PlayerStatsFetcher::new(league)?.with_season(s),
        None => player_stats::PlayerStatsFetcher::new(league)?,
    };
    fetcher.fetch_team_players(league, &team_id).await
}

/// Fetch season leaders as a categorized map.
/// Returns stat categories with top players in each.
#[tauri::command]
pub async fn fetch_season_leaders_map(
    league: String,
    season: Option<u32>,
) -> Result<player_stats::SeasonLeaders, String> {
    let league = league.parse::<live_data::SportLeague>()
        .map_err(|e| AppError::Validation(format!("Invalid league: {}", e)))?;
    let fetcher = match season {
        Some(s) => player_stats::PlayerStatsFetcher::new(league)?.with_season(s),
        None => player_stats::PlayerStatsFetcher::new(league)?,
    };
    fetcher.fetch_season_leaders_map(league).await
}

/// Fetch live player stats for multiple leagues concurrently.
/// Useful for getting a cross-sport overview.
#[tauri::command]
pub async fn fetch_multi_sport_leaders(
    leagues: Vec<String>,
    season: Option<u32>,
) -> Result<serde_json::Value, String> {
    use futures::future::join_all;

    let futures = leagues.into_iter().map(|league_str| {
        let season = season;
        async move {
            let league = match league_str.parse::<live_data::SportLeague>() {
                Ok(l) => l,
                Err(e) => return (league_str, Err(String::from(AppError::Validation(format!("Invalid league: {}", e))))),
            };
            let fetcher = match season {
                Some(s) => player_stats::PlayerStatsFetcher::new(league).unwrap().with_season(s),
                None => player_stats::PlayerStatsFetcher::new(league).unwrap(),
            };
            let result = fetcher.fetch_season_leaders(league).await.map_err(String::from);
            (league.short_name().to_string(), result)
        }
    });

    let results: Vec<(String, Result<Vec<player_stats::PlayerStatProfile>, String>)> =
        join_all(futures).await;

    let mut map = serde_json::Map::new();
    for (name, result) in results {
        match result {
            Ok(profiles) => {
                map.insert(name, serde_json::to_value(profiles).unwrap_or_default());
            }
            Err(e) => {
                map.insert(name, serde_json::json!({ "error": e }));
            }
        }
    }

    Ok(serde_json::Value::Object(map))
}

/// Build a live player stats context string for AI injection.
/// This fetches real-time stats and formats them for the AI prompt.
#[tauri::command]
pub async fn build_live_player_context(
    league: String,
    max_players: Option<usize>,
) -> Result<String, String> {
    let league = league.parse::<live_data::SportLeague>()
        .map_err(|e| AppError::Validation(format!("Invalid league: {}", e)))?;
    let fetcher = player_stats::PlayerStatsFetcher::new(league)?;
    let profiles = fetcher.fetch_season_leaders(league).await?;
    let max = max_players.unwrap_or(15);
    Ok(player_stats::build_live_player_context(&profiles, league, max))
}

