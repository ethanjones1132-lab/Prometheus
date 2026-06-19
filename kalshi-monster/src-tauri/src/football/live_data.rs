#![allow(dead_code)]
use crate::football::data::{FootballContext, GameInfo, PlayerProfile, MultiSportContext, get_football_context, get_multi_sport_context};
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveDataSnapshot {
    pub generated_at: String,
    pub query: String,
    pub schedule: Vec<GameInfo>,
    pub relevant_players: Vec<LivePlayerSnapshot>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivePlayerSnapshot {
    pub name: String,
    pub position: String,
    pub team: String,
    pub season_avg_game: Vec<(String, f64)>,
    pub last_3_avg: Vec<(String, f64)>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveDataConfig {
    pub espn_api_base: String,
    pub sleeper_api_base: String,
    pub sportsdata_api_key: String,
}

impl Default for LiveDataConfig {
    fn default() -> Self {
        Self {
            espn_api_base: "https://site.api.espn.com/apis/site/v2/sports/football/nfl".into(),
            sleeper_api_base: "https://api.sleeper.app/v1".into(),
            sportsdata_api_key: String::new(),
        }
    }
}

/// Supported sports leagues for live data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SportLeague {
    NFL,
    NBA,
    MLB,
    NHL,
}

impl SportLeague {
    /// ESPN API path segment for this league
    pub fn espn_path(&self) -> &'static str {
        match self {
            SportLeague::NFL => "football/nfl",
            SportLeague::NBA => "basketball/nba",
            SportLeague::MLB => "baseball/mlb",
            SportLeague::NHL => "hockey/nhl",
        }
    }

    /// Short display name
    pub fn short_name(&self) -> &'static str {
        match self {
            SportLeague::NFL => "NFL",
            SportLeague::NBA => "NBA",
            SportLeague::MLB => "MLB",
            SportLeague::NHL => "NHL",
        }
    }
}

impl std::str::FromStr for SportLeague {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "NFL" => Ok(SportLeague::NFL),
            "NBA" => Ok(SportLeague::NBA),
            "MLB" => Ok(SportLeague::MLB),
            "NHL" => Ok(SportLeague::NHL),
            _ => Err(format!("Unknown league: {}", s)),
        }
    }
}

/// Fetch the live scoreboard for any supported sport league
pub async fn fetch_live_scoreboard_for_league(league: SportLeague) -> Result<Vec<GameInfo>, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let url = format!(
        "https://site.api.espn.com/apis/site/v2/sports/{}/scoreboard",
        league.espn_path()
    );

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch {} scoreboard: {}", league.short_name(), e))?;

    if !response.status().is_success() {
        return Err(format!(
            "{} scoreboard returned HTTP {}",
            league.short_name(),
            response.status()
        ));
    }

    let payload: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse {} scoreboard JSON: {}", league.short_name(), e))?;

    Ok(parse_scoreboard(&payload))
}

/// Fetch live scoreboards for all supported leagues concurrently
pub async fn fetch_all_league_scoreboards() -> HashMap<SportLeague, Vec<GameInfo>> {
    let leagues = [SportLeague::NFL, SportLeague::NBA, SportLeague::MLB, SportLeague::NHL];
    let futures = leagues.iter().map(|&league| async move {
        let result = fetch_live_scoreboard_for_league(league).await;
        (league, result.unwrap_or_default())
    });

    let results: Vec<(SportLeague, Vec<GameInfo>)> = futures::future::join_all(futures).await;
    results.into_iter().collect()
}

/// Detect which sport league a user query is about
pub fn detect_league_from_query(query: &str) -> Option<SportLeague> {
    let lower = query.to_lowercase();
    // Check for explicit league mentions
    if lower.contains("nba") || lower.contains("basketball") {
        return Some(SportLeague::NBA);
    }
    if lower.contains("mlb") || lower.contains("baseball") {
        return Some(SportLeague::MLB);
    }
    if lower.contains("nhl") || lower.contains("hockey") {
        return Some(SportLeague::NHL);
    }
    if lower.contains("nfl") || lower.contains("football") {
        return Some(SportLeague::NFL);
    }
    None
}

/// Build a multi-sport live data context string for AI injection
pub async fn build_multi_sport_context(query_text: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "🏆 MULTI-SPORT LIVE DATA PACKET");
    let _ = writeln!(out, "Generated: {}", Utc::now().to_rfc3339());
    let _ = writeln!(out, "Query: {}", summarize_query(query_text));

    // Detect which league the user is asking about, or show all
    let target_league = detect_league_from_query(query_text);

    let leagues_to_fetch: Vec<SportLeague> = if let Some(league) = target_league {
        vec![league]
    } else {
        vec![SportLeague::NFL, SportLeague::NBA, SportLeague::MLB, SportLeague::NHL]
    };

    let futures = leagues_to_fetch.iter().map(|&league| async move {
        let result = fetch_live_scoreboard_for_league(league).await;
        (league, result)
    });

    let results: Vec<(SportLeague, Result<Vec<GameInfo>, String>)> =
        futures::future::join_all(futures).await;

    for (league, result) in results {
        match result {
            Ok(games) if !games.is_empty() => {
                let _ = writeln!(out, "\n📊 {} SCHEDULE ({} games):", league.short_name(), games.len());
                for game in games.iter().take(8) {
                    let _ = writeln!(
                        out,
                        "  {} @ {} | {} | total {:?} spread {:?}",
                        game.away_team, game.home_team, game.game_time,
                        game.total_line, game.spread,
                    );
                }
            }
            Ok(_) => {
                let _ = writeln!(out, "\n📊 {}: No games currently scheduled", league.short_name());
            }
            Err(e) => {
                let _ = writeln!(out, "\n📊 {}: Unavailable ({})", league.short_name(), e);
            }
        }
    }

    // Dynamic player / stats context injection from get_multi_sport_context
    if let Some(league) = target_league {
        if let Some(ctx) = get_multi_sport_context(league.short_name()) {
            let relevant_players = select_relevant_multi_sport_players(&ctx, query_text, 8);
            if !relevant_players.is_empty() {
                let _ = writeln!(out, "\n🎯 RELEVANT {} PLAYER CONTEXT:", league.short_name());
                for player in &relevant_players {
                    let _ = writeln!(out, "  {} ({}) {} — {}", player.name, player.team, player.position, player.note);
                    let _ = writeln!(out, "    season: {}", format_stat_pairs(&player.season_avg_game));
                    let _ = writeln!(out, "    last3: {}", format_stat_pairs(&player.last_3_avg));
                }
            }
            
            let _ = writeln!(out, "\n🛡️ KEY {} TEAM RANKS:", league.short_name());
            for r in ctx.team_rankings.iter().take(8) {
                let _ = writeln!(out, "  {} off#{}/def#{}/pace#{}: {}", r.team, r.offense_rank, r.defense_rank, r.pace_rank, r.note);
            }
            
            let _ = writeln!(out, "\n📊 {} NARRATIVES:", league.short_name());
            for note in ctx.trending_narratives.iter().take(8) {
                let _ = writeln!(out, "  - {}", note);
            }
        }
    }

    out
}

fn select_relevant_multi_sport_players(ctx: &MultiSportContext, query_text: &str, max_players: usize) -> Vec<LivePlayerSnapshot> {
    let tokens = tokenize(query_text);
    let mut ranked: Vec<(i32, &PlayerProfile)> = ctx.top_players.iter()
        .map(|player| (score_player(player, &tokens), player))
        .collect();

    ranked.sort_by_key(|(score, player)| (Reverse(*score), player.name.clone()));

    let mut selected = Vec::new();
    for (score, player) in ranked {
        if selected.len() >= max_players { break; }
        if score > 0 || selected.is_empty() {
            selected.push(to_snapshot(player));
        }
    }

    if selected.is_empty() {
        selected.extend(
            ctx.top_players.iter()
                .take(max_players)
                .map(to_snapshot),
        );
    }
    selected
}

pub async fn fetch_live_schedule() -> Result<Vec<GameInfo>, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let response = client
        .get("https://site.api.espn.com/apis/site/v2/sports/football/nfl/scoreboard")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch scoreboard: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Scoreboard returned HTTP {}", response.status()));
    }

    let payload: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse scoreboard JSON: {}", e))?;

    Ok(parse_scoreboard(&payload))
}

pub async fn build_live_data_context(query_text: &str, max_players: usize) -> String {
    // Check if the query is about a non-NFL sport
    if let Some(league) = detect_league_from_query(query_text) {
        if league != SportLeague::NFL {
            return build_multi_sport_context(query_text).await;
        }
    }

    // Default: NFL-focused context (original behavior)
    let context = get_football_context();
    let mut out = String::new();
    let _ = writeln!(out, "LIVE DATA PACKET v3");
    let _ = writeln!(out, "Generated: {}", Utc::now().to_rfc3339());
    let _ = writeln!(out, "Query focus: {}", summarize_query(query_text));

    match fetch_live_schedule().await {
        Ok(schedule) if !schedule.is_empty() => {
            let _ = writeln!(out, "\n📡 LIVE SCHEDULE:");
            for game in schedule.iter().take(16) {
                let _ = writeln!(
                    out,
                    "  {} @ {} | {} | total {:?} spread {:?}",
                    game.away_team, game.home_team, game.game_time, game.total_line, game.spread,
                );
            }
        }
        Ok(_) => { let _ = writeln!(out, "\n📡 LIVE SCHEDULE: no games returned by the current scoreboard."); }
        Err(err) => { let _ = writeln!(out, "\n📡 LIVE SCHEDULE: unavailable ({})", err); }
    }

    let relevant_players = select_relevant_players(&context, query_text, max_players);
    let _ = writeln!(out, "\n🎯 RELEVANT PLAYER CONTEXT:");
    if relevant_players.is_empty() {
        let _ = writeln!(out, "  No query match found; using the built-in top-player reference set.");
    }
    for player in &relevant_players {
        let _ = writeln!(out, "  {} ({}) {} — {}", player.name, player.team, player.position, player.note);
        let _ = writeln!(out, "    season: {}", format_stat_pairs(&player.season_avg_game));
        let _ = writeln!(out, "    last3: {}", format_stat_pairs(&player.last_3_avg));
    }

    let _ = writeln!(out, "\n🛡️ KEY DEFENSE RANKS:");
    for defense in context.defense_rankings.iter().take(8) {
        let _ = writeln!(
            out,
            "  {} pass#{}/rush#{}/pts#{}: {}",
            defense.team, defense.pass_def_rank, defense.rush_def_rank,
            defense.points_allowed_rank, defense.note,
        );
    }

    let _ = writeln!(out, "\n⚡ KEY TEAM OFFENSE RANKS:");
    for team in context.team_offense_rankings.iter().take(8) {
        let _ = writeln!(
            out,
            "  {} pts#{}/pass#{}/rush#{}/pace#{} [{}]: {}",
            team.team, team.points_rank, team.pass_yds_rank, team.rush_yds_rank,
            team.pace_rank, team.play_type, team.note,
        );
    }

    let _ = writeln!(out, "\n📊 PRIORITY NARRATIVES:");
    for note in context.trending_narratives.iter().take(8) {
        let _ = writeln!(out, "  - {}", note);
    }

    out
}

fn summarize_query(query_text: &str) -> String {
    let tokens = tokenize(query_text);
    if tokens.is_empty() { return "general NFL prop analysis".into(); }
    tokens.into_iter().take(10).collect::<Vec<_>>().join(", ")
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '\'' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            if current.len() > 1 { tokens.push(current.clone()); }
            current.clear();
        }
    }
    if !current.is_empty() && current.len() > 1 { tokens.push(current); }
    tokens
}

fn select_relevant_players(context: &FootballContext, query_text: &str, max_players: usize) -> Vec<LivePlayerSnapshot> {
    let tokens = tokenize(query_text);
    let mut ranked: Vec<(i32, &PlayerProfile)> = context.top_qbs.iter()
        .chain(context.top_rbs.iter())
        .chain(context.top_wrs.iter())
        .chain(context.top_tes.iter())
        .filter(|p| p.name != "Brian Thomas Jr.") // Skip placeholder
        .map(|player| (score_player(player, &tokens), player))
        .collect();

    ranked.sort_by_key(|(score, player)| (Reverse(*score), player.name.clone()));

    let mut selected = Vec::new();
    for (score, player) in ranked {
        if selected.len() >= max_players { break; }
        if score > 0 || selected.is_empty() {
            selected.push(to_snapshot(player));
        }
    }

    if selected.is_empty() {
        selected.extend(
            context.top_qbs.iter()
                .chain(context.top_rbs.iter())
                .chain(context.top_wrs.iter())
                .chain(context.top_tes.iter())
                .filter(|p| p.name != "Brian Thomas Jr.")
                .take(max_players)
                .map(to_snapshot),
        );
    }
    selected
}

fn score_player(player: &PlayerProfile, tokens: &[String]) -> i32 {
    if tokens.is_empty() { return 1; }
    let name = player.name.to_lowercase();
    let team = player.team.to_lowercase();
    let position = player.position.to_lowercase();
    let notes = player.notes.to_lowercase();
    let mut score = 0;
    for token in tokens {
        if token.len() < 2 { continue; }
        if name.contains(token) { score += 8; }
        if team.contains(token) { score += 5; }
        if position == *token { score += 4; }
        if notes.contains(token) { score += 1; }
    }
    score
}

fn to_snapshot(player: &PlayerProfile) -> LivePlayerSnapshot {
    LivePlayerSnapshot {
        name: player.name.clone(),
        position: player.position.clone(),
        team: player.team.clone(),
        season_avg_game: sorted_pairs(&player.season_avg_game),
        last_3_avg: sorted_pairs(&player.last_3_avg),
        note: player.notes.clone(),
    }
}

fn sorted_pairs(map: &HashMap<String, f64>) -> Vec<(String, f64)> {
    let mut pairs: Vec<_> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

fn format_stat_pairs(pairs: &[(String, f64)]) -> String {
    if pairs.is_empty() { return "none".into(); }
    pairs.iter().map(|(k, v)| format!("{}={:.1}", k, v)).collect::<Vec<_>>().join(", ")
}

fn parse_scoreboard(payload: &Value) -> Vec<GameInfo> {
    let Some(events) = payload.get("events").and_then(|v| v.as_array()) else { return Vec::new(); };
    events.iter().filter_map(parse_event).collect()
}

fn parse_event(event: &Value) -> Option<GameInfo> {
    let competitions = event.get("competitions")?.as_array()?;
    let competition = competitions.first()?;
    let competitors = competition.get("competitors")?.as_array()?;

    let mut home_team = None;
    let mut away_team = None;
    for competitor in competitors {
        let home_away = competitor.get("homeAway")?.as_str()?;
        let team = competitor.get("team")?;
        let abbr = team.get("abbreviation").and_then(|v| v.as_str())
            .or_else(|| team.get("displayName").and_then(|v| v.as_str()))
            .unwrap_or("UNK").to_string();
        match home_away {
            "home" => home_team = Some(abbr),
            "away" => away_team = Some(abbr),
            _ => {}
        }
    }

    let status = competition.get("status")
        .and_then(|s| s.get("type"))
        .and_then(|t| t.get("shortDetail"))
        .and_then(|v| v.as_str())
        .or_else(|| event.get("date").and_then(|v| v.as_str()))
        .unwrap_or("TBD").to_string();

    // Try to extract betting lines
    let providers = competition.get("providers").and_then(|p| p.as_array());
    let mut total_line = None;
    let mut spread = None;
    if let Some(providers) = providers {
        for provider in providers {
            if let Some(records) = provider.get("records").and_then(|r| r.as_array()) {
                for record in records {
                    if let Some(val) = record.get("total").and_then(|v| v.as_f64()) {
                        total_line = Some(val as f32);
                    }
                    if let Some(val) = record.get("spread").and_then(|v| v.as_f64()) {
                        spread = Some(val as f32);
                    }
                }
            }
        }
    }

    Some(GameInfo {
        home_team: home_team?, away_team: away_team?, game_time: status,
        total_line, spread,
    })
}
