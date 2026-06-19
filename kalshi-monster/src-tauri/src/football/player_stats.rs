#![allow(dead_code)]
//! ═══════════════════════════════════════════════════════════════
//! Live Player Stats — Multi-Sport Player Data Ingestion
//!
//! Fetches real-time player statistics from ESPN's core API
//! for NBA, MLB, NHL, and NFL. Normalizes sport-specific stats
//! into a common PlayerStatProfile format for AI context injection.
//!
//! Data sources:
//!   - ESPN Core API (free, no key): player stats, season leaders
//!   - Sport-specific stat categories are mapped to common fields
//! ═══════════════════════════════════════════════════════════════

use crate::football::api_client::SportsApiClient;
use crate::football::live_data::SportLeague;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write as _;
// ── Normalized Player Stat Profile ──

/// A sport-agnostic player stat profile for AI context injection.
/// All stats are normalized to per-game averages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStatProfile {
    pub name: String,
    pub team: String,
    pub position: String,
    pub sport: String,
    pub season: u32,
    pub games_played: u32,
    /// Normalized per-game stats (sport-specific keys)
    pub per_game_stats: HashMap<String, f64>,
    /// Last 5 games per-game stats
    pub last_5_stats: HashMap<String, f64>,
    /// Season totals
    pub season_totals: HashMap<String, f64>,
    /// Stat categories this player leads in (for AI context)
    pub league_leads: Vec<String>,
    /// Data freshness timestamp
    pub fetched_at: String,
    /// Source URL
    pub source: String,
}

/// A collection of player profiles for a team/league
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamPlayerStats {
    pub team: String,
    pub sport: String,
    pub season: u32,
    pub players: Vec<PlayerStatProfile>,
    pub fetched_at: String,
}

/// League-wide season leaders in key stat categories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonLeaders {
    pub sport: String,
    pub season: u32,
    /// Category name -> (player_name, stat_value)
    pub categories: HashMap<String, Vec<(String, f64)>>,
    pub fetched_at: String,
}

// ── Sport-specific stat category mappings ──

/// ESPN stat category IDs mapped to human-readable names for NBA
const NBA_STAT_CATEGORIES: &[(&str, &str)] = &[
    ("0", "points"),
    ("1", "rebounds"),
    ("2", "assists"),
    ("3", "steals"),
    ("4", "blocks"),
    ("5", "turnovers"),
    ("6", "fg_pct"),
    ("7", "ft_pct"),
    ("8", "three_pm"),
    ("9", "minutes"),
    ("10", "games_played"),
    ("11", "games_started"),
    ("12", "off_reb"),
    ("13", "def_reb"),
    ("14", "pf"),
    ("15", "plus_minus"),
];

/// ESPN stat category IDs mapped to human-readable names for MLB
const MLB_STAT_CATEGORIES: &[(&str, &str)] = &[
    ("0", "avg"),
    ("1", "hr"),
    ("2", "rbi"),
    ("3", "runs"),
    ("4", "sb"),
    ("5", "obp"),
    ("6", "slg"),
    ("7", "ops"),
    ("8", "hits"),
    ("9", "doubles"),
    ("10", "triples"),
    ("11", "bb"),
    ("12", "so"),
    ("13", "games"),
    ("14", "at_bats"),
    ("15", "war"),
];

/// ESPN stat category IDs mapped to human-readable names for NHL
const NHL_STAT_CATEGORIES: &[(&str, &str)] = &[
    ("0", "goals"),
    ("1", "assists"),
    ("2", "points"),
    ("3", "sog"),
    ("4", "plus_minus"),
    ("5", "pim"),
    ("6", "pp_points"),
    ("7", "sh_points"),
    ("8", "gw_goals"),
    ("9", "ot_goals"),
    ("10", "shots_pct"),
    ("11", "games"),
    ("12", "hits"),
    ("13", "blocks"),
    ("14", "faceoff_pct"),
    ("15", "toi_per_game"),
];

/// ESPN stat category IDs mapped to human-readable names for NFL
const NFL_STAT_CATEGORIES: &[(&str, &str)] = &[
    ("0", "pass_yds"),
    ("1", "pass_td"),
    ("2", "pass_int"),
    ("3", "rush_yds"),
    ("4", "rush_td"),
    ("5", "receptions"),
    ("6", "rec_yds"),
    ("7", "rec_td"),
    ("8", "fumbles"),
    ("9", "games"),
    ("10", "games_started"),
    ("11", "targets"),
    ("12", "yards_after_catch"),
    ("13", "air_yards"),
    ("14", "carries"),
    ("15", "fantasy_pts"),
];

fn get_stat_categories(league: SportLeague) -> &'static [(&'static str, &'static str)] {
    match league {
        SportLeague::NFL => NFL_STAT_CATEGORIES,
        SportLeague::NBA => NBA_STAT_CATEGORIES,
        SportLeague::MLB => MLB_STAT_CATEGORIES,
        SportLeague::NHL => NHL_STAT_CATEGORIES,
    }
}

/// Get the ESPN sport path for a league
fn espn_sport_path(league: SportLeague) -> &'static str {
    league.espn_path()
}

/// Get the current season year for a league
fn current_season(league: SportLeague) -> u32 {
    // 2025 is the current season for all sports
    // NBA/NHL: 2024-25 season, MLB: 2025, NFL: 2025
    match league {
        SportLeague::NBA | SportLeague::NHL => 2025,
        SportLeague::MLB | SportLeague::NFL => 2025,
    }
}

// ── Player Stats Fetcher ──

pub struct PlayerStatsFetcher {
    client: SportsApiClient,
    season: u32,
}

impl PlayerStatsFetcher {
    pub fn new(league: SportLeague) -> Result<Self, String> {
        let api_config = crate::football::api_client::SportsApiConfig::default();
        let client = SportsApiClient::new(api_config)?;
        Ok(Self {
            client,
            season: current_season(league),
        })
    }

    pub fn with_season(mut self, season: u32) -> Self {
        self.season = season;
        self
    }

    /// Fetch season leaders for a league and return normalized profiles.
    /// This is the primary entry point for getting top player data.
    pub async fn fetch_season_leaders(
        &self,
        league: SportLeague,
    ) -> Result<Vec<PlayerStatProfile>, String> {
        let sport_path = espn_sport_path(league);
        let leaders = self
            .client
            .espn_sport_season_leaders(sport_path, self.season)
            .await?;

        Ok(parse_season_leaders(&leaders, league, self.season))
    }

    /// Fetch a specific player's stats by athlete ID.
    pub async fn fetch_player_stats(
        &self,
        league: SportLeague,
        athlete_id: &str,
    ) -> Result<PlayerStatProfile, String> {
        let sport_path = espn_sport_path(league);
        let stats = self
            .client
            .espn_sport_player_stats(sport_path, athlete_id, self.season)
            .await?;

        parse_player_stats(&stats, league, self.season, athlete_id)
            .ok_or_else(|| format!("Failed to parse player stats for athlete {}", athlete_id))
    }

    /// Fetch all players for a team.
    pub async fn fetch_team_players(
        &self,
        league: SportLeague,
        team_id: &str,
    ) -> Result<TeamPlayerStats, String> {
        let sport_path = espn_sport_path(league);
        let roster = self
            .client
            .espn_sport_team_roster(sport_path, team_id, self.season)
            .await?;

        Ok(parse_team_roster(&roster, league, team_id, self.season))
    }

    /// Fetch season leaders and return as a SeasonLeaders struct.
    pub async fn fetch_season_leaders_map(
        &self,
        league: SportLeague,
    ) -> Result<SeasonLeaders, String> {
        let sport_path = espn_sport_path(league);
        let leaders = self
            .client
            .espn_sport_season_leaders(sport_path, self.season)
            .await?;

        Ok(parse_season_leaders_map(&leaders, league, self.season))
    }
}

// ── Parsing Functions ──

/// Parse ESPN season leaders response into normalized player profiles.
fn parse_season_leaders(payload: &Value, league: SportLeague, season: u32) -> Vec<PlayerStatProfile> {
    let categories = get_stat_categories(league);
    let mut profiles = Vec::new();

    // ESPN leaders response structure:
    // { "categories": [ { "name": "...", "displayName": "...", "leaders": [ { "athlete": {...}, "team": {...}, "statValue": ... } ] } ] }
    let Some(cats) = payload.get("categories").and_then(|c| c.as_array()) else {
        return profiles;
    };

    for cat in cats {
        let Some(leaders) = cat.get("leaders").and_then(|l| l.as_array()) else {
            continue;
        };

        // Find the stat name from our mapping
        let cat_name = cat
            .get("name")
            .or_else(|| cat.get("displayName"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let stat_key = find_stat_key(cat_name, categories);

        for leader in leaders.iter().take(10) {
            let athlete = leader.get("athlete");
            let name = athlete
                .and_then(|a| a.get("displayName"))
                .or_else(|| athlete.and_then(|a| a.get("fullName")))
                .or_else(|| athlete.and_then(|a| a.get("name")))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");

            let team = leader
                .get("team")
                .and_then(|t| t.get("abbreviation"))
                .or_else(|| leader.get("team").and_then(|t| t.get("displayName")))
                .and_then(|v| v.as_str())
                .unwrap_or("UNK");

            let position = athlete
                .and_then(|a| a.get("position"))
                .and_then(|p| p.get("abbreviation"))
                .or_else(|| athlete.and_then(|a| a.get("position")))
                .and_then(|v| v.as_str())
                .unwrap_or("UNK");

            let stat_val = leader
                .get("displayValue")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| leader.get("value").and_then(|v| v.as_f64()))
                .unwrap_or(0.0);

            // Find or create profile for this player
            if let Some(existing) = profiles.iter_mut().find(|p| p.name == name) {
                if !stat_key.is_empty() {
                    existing.per_game_stats.insert(stat_key.to_string(), stat_val);
                }
                existing.league_leads.push(cat_name.to_string());
            } else {
                let mut stats = HashMap::new();
                if !stat_key.is_empty() {
                    stats.insert(stat_key.to_string(), stat_val);
                }
                profiles.push(PlayerStatProfile {
                    name: name.to_string(),
                    team: team.to_string(),
                    position: position.to_string(),
                    sport: league.short_name().to_string(),
                    season,
                    games_played: 0,
                    per_game_stats: stats,
                    last_5_stats: HashMap::new(),
                    season_totals: HashMap::new(),
                    league_leads: vec![cat_name.to_string()],
                    fetched_at: Utc::now().to_rfc3339(),
                    source: format!("ESPN {}", league.short_name()),
                });
            }
        }
    }

    profiles
}

/// Parse ESPN season leaders into a SeasonLeaders map.
fn parse_season_leaders_map(
    payload: &Value,
    league: SportLeague,
    season: u32,
) -> SeasonLeaders {
    let mut categories = HashMap::new();

    let Some(cats) = payload.get("categories").and_then(|c| c.as_array()) else {
        return SeasonLeaders {
            sport: league.short_name().to_string(),
            season,
            categories,
            fetched_at: Utc::now().to_rfc3339(),
        };
    };

    for cat in cats {
        let cat_name = cat
            .get("displayName")
            .or_else(|| cat.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut leaders_vec = Vec::new();
        if let Some(leaders) = cat.get("leaders").and_then(|l| l.as_array()) {
            for leader in leaders.iter().take(15) {
                let name = leader
                    .get("athlete")
                    .and_then(|a| a.get("displayName"))
                    .or_else(|| leader.get("athlete").and_then(|a| a.get("fullName")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");

                let val = leader
                    .get("displayValue")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok())
                    .or_else(|| leader.get("value").and_then(|v| v.as_f64()))
                    .unwrap_or(0.0);

                leaders_vec.push((name.to_string(), val));
            }
        }

        if !leaders_vec.is_empty() {
            categories.insert(cat_name.to_string(), leaders_vec);
        }
    }

    SeasonLeaders {
        sport: league.short_name().to_string(),
        season,
        categories,
        fetched_at: Utc::now().to_rfc3339(),
    }
}

/// Parse ESPN player statistics response into a normalized profile.
fn parse_player_stats(
    payload: &Value,
    league: SportLeague,
    season: u32,
    athlete_id: &str,
) -> Option<PlayerStatProfile> {
    let categories = get_stat_categories(league);

    // Extract athlete info
    let athlete = payload.get("athlete").or_else(|| payload.get("player"));
    let name = athlete
        .and_then(|a| a.get("displayName"))
        .or_else(|| athlete.and_then(|a| a.get("fullName")))
        .or_else(|| athlete.and_then(|a| a.get("name")))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let team = payload
        .get("team")
        .and_then(|t| t.get("abbreviation"))
        .or_else(|| payload.get("team").and_then(|t| t.get("displayName")))
        .and_then(|v| v.as_str())
        .unwrap_or("UNK");

    let position = athlete
        .and_then(|a| a.get("position"))
        .and_then(|p| p.get("abbreviation"))
        .or_else(|| athlete.and_then(|a| a.get("position")))
        .and_then(|v| v.as_str())
        .unwrap_or("UNK");

    // Extract stats from the splits/categories structure
    let mut per_game_stats = HashMap::new();
    let mut season_totals = HashMap::new();
    let mut games_played = 0u32;

    // ESPN stats structure varies by sport, but generally:
    // { "splits": { "categories": [ { "name": "...", "stats": [ { "name": "...", "value": ..., "abbreviation": "..." } ] } ] } }
    if let Some(splits) = payload.get("splits") {
        if let Some(cats) = splits.get("categories").and_then(|c| c.as_array()) {
            for cat in cats {
                let _cat_name = cat.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(stats) = cat.get("stats").and_then(|s| s.as_array()) {
                    for stat in stats {
                        let stat_name = stat
                            .get("name")
                            .or_else(|| stat.get("abbreviation"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        let mapped_key = map_espn_stat_name(stat_name, categories);

                        if let Some(val) = stat.get("value").and_then(|v| v.as_f64()) {
                            if stat_name.contains("avg") || stat_name.contains("perGame") {
                                per_game_stats.insert(mapped_key.clone(), val);
                            } else {
                                season_totals.insert(mapped_key.clone(), val);
                            }
                        }

                        // Extract games played
                        if stat_name == "gamesPlayed" || stat_name == "games" {
                            if let Some(gp) = stat.get("value").and_then(|v| v.as_u64()) {
                                games_played = gp as u32;
                            }
                        }
                    }
                }
            }
        }
    }

    // Alternative: direct stats array
    if per_game_stats.is_empty() {
        if let Some(stats) = payload.get("stats").and_then(|s| s.as_array()) {
            for stat in stats {
                let stat_name = stat
                    .get("name")
                    .or_else(|| stat.get("abbreviation"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let mapped_key = map_espn_stat_name(stat_name, categories);

                if let Some(val) = stat.get("value").and_then(|v| v.as_f64()) {
                    per_game_stats.insert(mapped_key, val);
                }
            }
        }
    }

    Some(PlayerStatProfile {
        name: name.to_string(),
        team: team.to_string(),
        position: position.to_string(),
        sport: league.short_name().to_string(),
        season,
        games_played,
        per_game_stats,
        last_5_stats: HashMap::new(),
        season_totals,
        league_leads: Vec::new(),
        fetched_at: Utc::now().to_rfc3339(),
        source: format!("ESPN {} athlete/{}", league.short_name(), athlete_id),
    })
}

/// Parse ESPN team roster response.
fn parse_team_roster(
    payload: &Value,
    league: SportLeague,
    team_id: &str,
    season: u32,
) -> TeamPlayerStats {
    let mut players = Vec::new();

    let team_name = payload
        .get("team")
        .and_then(|t| t.get("abbreviation"))
        .or_else(|| payload.get("team").and_then(|t| t.get("displayName")))
        .and_then(|v| v.as_str())
        .unwrap_or(team_id);

    // Roster is typically an array of athlete references
    if let Some(athletes) = payload.get("athletes").and_then(|a| a.as_array()) {
        for athlete in athletes {
            let name = athlete
                .get("displayName")
                .or_else(|| athlete.get("fullName"))
                .or_else(|| athlete.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");

            let position = athlete
                .get("position")
                .and_then(|p| p.get("abbreviation"))
                .or_else(|| athlete.get("position"))
                .and_then(|v| v.as_str())
                .unwrap_or("UNK");

            let id = athlete
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            players.push(PlayerStatProfile {
                name: name.to_string(),
                team: team_name.to_string(),
                position: position.to_string(),
                sport: league.short_name().to_string(),
                season,
                games_played: 0,
                per_game_stats: HashMap::new(),
                last_5_stats: HashMap::new(),
                season_totals: HashMap::new(),
                league_leads: Vec::new(),
                fetched_at: Utc::now().to_rfc3339(),
                source: format!("ESPN {} team/{}/athlete/{}", league.short_name(), team_id, id),
            });
        }
    }

    TeamPlayerStats {
        team: team_name.to_string(),
        sport: league.short_name().to_string(),
        season,
        players,
        fetched_at: Utc::now().to_rfc3339(),
    }
}

// ── Helper Functions ──

/// Map an ESPN stat name/abbreviation to our normalized key.
fn map_espn_stat_name(espn_name: &str, categories: &[(&str, &str)]) -> String {
    // First try direct match by abbreviation
    for &(abbr, key) in categories {
        if espn_name.eq_ignore_ascii_case(abbr) || espn_name.eq_ignore_ascii_case(key) {
            return key.to_string();
        }
    }

    // Common ESPN stat name mappings
    match espn_name.to_lowercase().as_str() {
        "pts" | "points" | "pts/g" => "points".to_string(),
        "reb" | "totalrebounds" | "rebounds" | "reb/g" => "rebounds".to_string(),
        "ast" | "assists" | "ast/g" => "assists".to_string(),
        "stl" | "steals" | "stl/g" => "steals".to_string(),
        "blk" | "blocks" | "blk/g" => "blocks".to_string(),
        "fg%" | "fieldgoalpercentage" | "fg_pct" => "fg_pct".to_string(),
        "ft%" | "freethrowpercentage" | "ft_pct" => "ft_pct".to_string(),
        "3pm" | "threepointfieldgoalsmade" | "3pm/g" => "three_pm".to_string(),
        "min" | "minutes" | "mpg" | "minutespergame" => "minutes".to_string(),
        "gp" | "gamesplayed" | "games" => "games_played".to_string(),
        "avg" | "battingaverage" => "avg".to_string(),
        "hr" | "homeruns" => "hr".to_string(),
        "rbi" => "rbi".to_string(),
        "sb" | "stolenbases" => "sb".to_string(),
        "obp" | "onbasepercentage" => "obp".to_string(),
        "slg" | "slugging" => "slg".to_string(),
        "sog" | "shotsongoal" | "shots" => "sog".to_string(),
        "g" | "goals" => "goals".to_string(),
        "passingyards" | "pass yds" | "passyds" => "pass_yds".to_string(),
        "passingtouchdowns" | "pass td" | "passtd" => "pass_td".to_string(),
        "rushingyards" | "rush yds" | "rushyds" => "rush_yds".to_string(),
        "rushingtouchdowns" | "rush td" | "rushtd" => "rush_td".to_string(),
        "receivingyards" | "rec yds" | "recyds" => "rec_yds".to_string(),
        "receivingtouchdowns" | "rec td" | "rectd" => "rec_td".to_string(),
        _ => espn_name.to_lowercase().replace(' ', "_"),
    }
}

/// Find the normalized stat key for a category display name.
fn find_stat_key(display_name: &str, _categories: &[(&str, &str)]) -> &'static str {
    let lower = display_name.to_lowercase();

    // Try to match by display name patterns
    if lower.contains("point") && !lower.contains("rebound") {
        return "points";
    }
    if lower.contains("rebound") {
        return "rebounds";
    }
    if lower.contains("assist") {
        return "assists";
    }
    if lower.contains("steal") {
        return "steals";
    }
    if lower.contains("block") {
        return "blocks";
    }
    if lower.contains("three") || lower.contains("3pt") {
        return "three_pm";
    }
    if lower.contains("batting average") || lower.contains("avg") {
        return "avg";
    }
    if lower.contains("home run") || lower.contains("hr") {
        return "hr";
    }
    if lower.contains("rbi") {
        return "rbi";
    }
    if lower.contains("stolen base") || lower.contains("sb") {
        return "sb";
    }
    if lower.contains("goal") && !lower.contains("goaltender") {
        return "goals";
    }
    if lower.contains("shot on goal") || lower.contains("sog") {
        return "sog";
    }
    if lower.contains("passing yard") {
        return "pass_yds";
    }
    if lower.contains("rushing yard") {
        return "rush_yds";
    }
    if lower.contains("receiving yard") {
        return "rec_yds";
    }

    ""
}

// ── AI Context Builder ──

/// Build a live player stats context string for AI injection.
/// This supplements the static player data with real-time stats.
pub fn build_live_player_context(
    profiles: &[PlayerStatProfile],
    league: SportLeague,
    max_players: usize,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\n📊 LIVE PLAYER STATS ({}, {} season):", league.short_name(), profiles.first().map(|p| p.season).unwrap_or(2025));
    let _ = writeln!(out, "Source: ESPN | Fetched: {}", Utc::now().format("%Y-%m-%d %H:%M UTC"));

    for profile in profiles.iter().take(max_players) {
        let _ = writeln!(out, "\n  {} ({}, {})", profile.name, profile.team, profile.position);

        if !profile.per_game_stats.is_empty() {
            let stats_str = format_live_stats(&profile.per_game_stats, league);
            let _ = writeln!(out, "    Per Game: {}", stats_str);
        }

        if !profile.season_totals.is_empty() {
            let totals_str = format_live_totals(&profile.season_totals, league);
            let _ = writeln!(out, "    Totals: {}", totals_str);
        }

        if !profile.league_leads.is_empty() {
            let _ = writeln!(out, "    League Leads: {}", profile.league_leads.join(", "));
        }
    }

    out
}

/// Format per-game stats for display based on sport.
fn format_live_stats(stats: &HashMap<String, f64>, league: SportLeague) -> String {
    let keys = match league {
        SportLeague::NBA => vec!["points", "rebounds", "assists", "steals", "blocks", "three_pm", "minutes"],
        SportLeague::MLB => vec!["avg", "hr", "rbi", "runs", "sb", "obp", "slg"],
        SportLeague::NHL => vec!["goals", "assists", "points", "sog", "plus_minus", "pim"],
        SportLeague::NFL => vec!["pass_yds", "pass_td", "rush_yds", "rush_td", "receptions", "rec_yds", "rec_td"],
    };

    let parts: Vec<String> = keys
        .iter()
        .filter_map(|k| stats.get(*k).map(|v| format!("{}={:.1}", k, v)))
        .collect();

    if parts.is_empty() {
        // Fallback: show all stats
        stats
            .iter()
            .map(|(k, v)| format!("{}={:.1}", k, v))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        parts.join(", ")
    }
}

/// Format season totals for display based on sport.
fn format_live_totals(stats: &HashMap<String, f64>, league: SportLeague) -> String {
    let keys = match league {
        SportLeague::NBA => vec!["games_played", "points", "rebounds", "assists"],
        SportLeague::MLB => vec!["games_played", "avg", "hr", "rbi", "sb"],
        SportLeague::NHL => vec!["games_played", "goals", "assists", "points", "sog"],
        SportLeague::NFL => vec!["games_played", "pass_yds", "pass_td", "rush_yds", "rush_td"],
    };

    let parts: Vec<String> = keys
        .iter()
        .filter_map(|k| stats.get(*k).map(|v| format!("{}={:.0}", k, v)))
        .collect();

    if parts.is_empty() {
        stats
            .iter()
            .map(|(k, v)| format!("{}={:.0}", k, v))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        parts.join(", ")
    }
}
