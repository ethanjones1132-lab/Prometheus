#![allow(dead_code)]
//! ═══════════════════════════════════════════════════════════════
//! Automated Prediction Grading via ESPN Boxscore API
//!
//! Fetches ESPN boxscore data for completed games and compares
//! actual player stats against prop lines to determine
//! Win/Loss/Push without manual input.
//! ═══════════════════════════════════════════════════════════════

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// The result of attempting to grade a single prediction
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GradingResult {
    pub prediction_id: String,
    pub player_name: String,
    pub stat_category: String,
    pub line: f64,
    pub pick_type: String,
    pub outcome: String,       // "Win", "Loss", "Push", "Unresolved"
    pub actual_result: Option<f64>,
    pub game_id: Option<String>,
    pub game_status: Option<String>, // "Final", "In Progress", "Not Found", etc.
    pub notes: String,
}

/// Summary of a grading run
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GradingSummary {
    pub total_pending: usize,
    pub graded: usize,
    pub skipped: usize,
    pub wins: usize,
    pub losses: usize,
    pub pushes: usize,
    pub unresolved: usize,
    pub results: Vec<GradingResult>,
    pub fetched_at: String,
}

/// ESPN stat category mapping
/// Maps our internal stat_category names to ESPN boxscore stat keys
const STAT_CATEGORY_MAP: &[(&str, &[&str])] = &[
    // (our_category, [espn_stat_keys...])
    ("Passing Yards", &["passingYards", "passYards", "passing yards"]),
    ("Rushing Yards", &["rushingYards", "rushYards", "rushing yards"]),
    ("Receiving Yards", &["receivingYards", "recYards", "receiving yards"]),
    ("Passing Touchdowns", &["passingTouchdowns", "passingTd", "passing touchdowns"]),
    ("Rushing Touchdowns", &["rushingTouchdowns", "rushingTd", "rushing touchdowns"]),
    ("Receiving Touchdowns", &["receivingTouchdowns", "receivingTd", "receiving touchdowns"]),
    ("Receptions", &["receptions", "receivingReceptions", "receptions"]),
    ("Passing Completions", &["completions", "passingCompletions", "completions"]),
    ("Passing Attempts", &["passingAttempts", "passAttempts", "passing attempts"]),
    ("Rushing Attempts", &["rushingAttempts", "rushAttempts", "rushing attempts", "carries"]),
    ("Interceptions", &["interceptions", "passingInterceptions", "interceptions"]),
    ("Longest Pass", &["longestPass", "longPass", "longest pass"]),
    ("Longest Rush", &["longestRush", "longRush", "longest rush"]),
    ("Longest Reception", &["longestReception", "longRec", "longest reception"]),
    ("Targets", &["targets", "receivingTargets", "targets"]),
    ("Total Touchdowns", &["totalTouchdowns", "totalTd", "total touchdowns"]),
    ("Total Yards", &["totalYards", "total yards", "yardsFromScrimmage"]),
];

/// Normalize a stat category string to a standard form
fn normalize_category(input: &str) -> String {
    let lower = input.trim().to_lowercase();
    for (canonical, aliases) in STAT_CATEGORY_MAP {
        for alias in *aliases {
            if lower == alias.to_string().to_lowercase() {
                return canonical.to_string();
            }
        }
    }
    // Return original if no match
    input.trim().to_string()
}

/// Find the ESPN stat key that matches a given category name
fn find_espn_stat_key(category: &str) -> Option<&'static str> {
    let normalized = normalize_category(category);
    for (canonical, aliases) in STAT_CATEGORY_MAP {
        if normalized == *canonical {
            return Some(aliases[0]); // Return primary key
        }
    }
    None
}

/// Fetch the ESPN scoreboard and return completed games with their IDs
pub async fn fetch_completed_games() -> Result<Vec<CompletedGame>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let url = "https://site.api.espn.com/apis/site/v2/sports/football/nfl/scoreboard";
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("ESPN scoreboard request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("ESPN scoreboard returned HTTP {}", resp.status()));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("ESPN scoreboard parse error: {}", e))?;

    let mut games = Vec::new();

    if let Some(events) = json.get("events").and_then(|v| v.as_array()) {
        for event in events {
            let game_id = event.get("id").and_then(|v| v.as_str()).map(String::from);

            let competitions = event
                .get("competitions")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first());

            if let Some(competition) = competitions {
                let status = competition
                    .get("status")
                    .and_then(|s| s.get("type"))
                    .and_then(|t| t.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("UNKNOWN");

                // Only process completed games (post-game status)
                let is_final = matches!(
                    status,
                    "STATUS_FINAL" | "STATUS_FULL_TIME" | "STATUS_FINAL_OVERTIME"
                );

                if !is_final {
                    continue;
                }

                let competitors = competition
                    .get("competitors")
                    .and_then(|v| v.as_array());

                let mut home_abbr = String::new();
                let mut away_abbr = String::new();
                let mut home_name = String::new();
                let mut away_name = String::new();

                if let Some(competitors) = competitors {
                    for comp in competitors {
                        let home_away = comp
                            .get("homeAway")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let abbr = comp
                            .get("team")
                            .and_then(|t| t.get("abbreviation"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("UNK");
                        let display_name = comp
                            .get("team")
                            .and_then(|t| t.get("displayName"))
                            .and_then(|v| v.as_str())
                            .unwrap_or(abbr);

                        match home_away {
                            "home" => {
                                home_abbr = abbr.to_string();
                                home_name = display_name.to_string();
                            }
                            "away" => {
                                away_abbr = abbr.to_string();
                                away_name = display_name.to_string();
                            }
                            _ => {}
                        }
                    }
                }

                if let Some(id) = game_id {
                    games.push(CompletedGame {
                        game_id: id,
                        home_team: home_abbr,
                        away_team: away_abbr,
                        home_name,
                        away_name,
                        status: status.to_string(),
                    });
                }
            }
        }
    }

    Ok(games)
}

/// A completed game from the scoreboard
#[derive(Debug, Clone)]
pub struct CompletedGame {
    pub game_id: String,
    pub home_team: String,
    pub away_team: String,
    pub home_name: String,
    pub away_name: String,
    pub status: String,
}

/// Fetch the boxscore for a completed game and extract player stats
pub async fn fetch_game_boxscore(
    game_id: &str,
) -> Result<HashMap<String, HashMap<String, f64>>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let url = format!(
        "https://site.api.espn.com/apis/site/v2/sports/football/nfl/summary?event={}",
        game_id
    );
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("ESPN boxscore request failed for game {}: {}", game_id, e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "ESPN boxscore returned HTTP {} for game {}",
            resp.status(),
            game_id
        ));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("ESPN boxscore parse error for game {}: {}", game_id, e))?;

    let mut player_stats: HashMap<String, HashMap<String, f64>> = HashMap::new();

    // Parse boxscore players
    // ESPN boxscore structure: boxscore -> players -> [team entries] -> statistics -> [stat categories]
    if let Some(players_section) = json
        .get("boxscore")
        .and_then(|b| b.get("players"))
        .and_then(|p| p.as_array())
    {
        for team_entry in players_section {
            if let Some(statistics) = team_entry
                .get("statistics")
                .and_then(|s| s.as_array())
            {
                for stat_category in statistics {
                    // Each stat category has "keys" (stat names) and "athletes" (player values)
                    let keys: Vec<String> = stat_category
                        .get("keys")
                        .and_then(|k| k.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();

                    if let Some(athletes) = stat_category
                        .get("athletes")
                        .and_then(|a| a.as_array())
                    {
                        for athlete in athletes {
                            let name = athlete
                                .get("athlete")
                                .and_then(|a| a.get("displayName"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown");

                            let stats = athlete
                                .get("stats")
                                .and_then(|s| s.as_array());

                            if let Some(stats) = stats {
                                let entry = player_stats
                                    .entry(name.to_string())
                                    .or_insert_with(HashMap::new);

                                for (i, stat_value) in stats.iter().enumerate() {
                                    if i < keys.len() {
                                        let key = &keys[i];
                                        if let Some(val) = stat_value.as_f64().or_else(|| {
                                            stat_value.as_str().and_then(|s| s.parse::<f64>().ok())
                                        }) {
                                            entry.insert(key.clone(), val);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Also try the "gamepackage" / "roster" structure (newer ESPN API format)
    if player_stats.is_empty() {
        if let Some(rosters) = json
            .get("gamepackage")
            .and_then(|gp| gp.get("roster"))
            .and_then(|r| r.as_array())
        {
            for team_roster in rosters {
                if let Some(players) = team_roster
                    .get("players")
                    .and_then(|p| p.as_array())
                {
                    for player in players {
                        let name = player
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown");

                        if let Some(stats_obj) = player.get("statistics").and_then(|s| s.as_object()) {
                            let entry = player_stats
                                .entry(name.to_string())
                                .or_insert_with(HashMap::new);
                            for (key, val) in stats_obj {
                                if let Some(num) = val.as_f64().or_else(|| {
                                    val.as_str().and_then(|s| s.parse::<f64>().ok())
                                }) {
                                    entry.insert(key.clone(), num);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(player_stats)
}

/// Look up a player's stat value from boxscore data.
/// Uses fuzzy name matching since ESPN names may differ slightly from PrizePicks names.
fn lookup_player_stat(
    boxscore: &HashMap<String, HashMap<String, f64>>,
    player_name: &str,
    stat_category: &str,
) -> Option<f64> {
    // Try exact match first
    if let Some(stats) = boxscore.get(player_name) {
        if let Some(espn_key) = find_espn_stat_key(stat_category) {
            if let Some(&val) = stats.get(espn_key) {
                return Some(val);
            }
        }
        // Try all keys in the stat category map
        let normalized = normalize_category(stat_category);
        for (canonical, aliases) in STAT_CATEGORY_MAP {
            if normalized == *canonical {
                for alias in *aliases {
                    if let Some(&val) = stats.get(*alias) {
                        return Some(val);
                    }
                }
            }
        }
        // Try direct key match with the stat category itself
        if let Some(&val) = stats.get(stat_category) {
            return Some(val);
        }
    }

    // Try case-insensitive partial match
    let name_lower = player_name.to_lowercase();
    for (espn_name, stats) in boxscore {
        let espn_lower = espn_name.to_lowercase();

        // Check if one contains the other
        if espn_lower.contains(&name_lower)
            || name_lower.contains(&espn_lower)
            || name_matches_fuzzy(&name_lower, &espn_lower)
        {
            if let Some(espn_key) = find_espn_stat_key(stat_category) {
                if let Some(&val) = stats.get(espn_key) {
                    return Some(val);
                }
            }
            // Try all aliases
            let normalized = normalize_category(stat_category);
            for (canonical, aliases) in STAT_CATEGORY_MAP {
                if normalized == *canonical {
                    for alias in *aliases {
                        if let Some(&val) = stats.get(*alias) {
                            return Some(val);
                        }
                    }
                }
            }
            if let Some(&val) = stats.get(stat_category) {
                return Some(val);
            }
        }
    }

    None
}

/// Fuzzy name matching for player names that may differ between sources
fn name_matches_fuzzy(name1: &str, name2: &str) -> bool {
    // Handle "Last, First" vs "First Last"
    let parts1: Vec<&str> = name1.split(|c| c == ',' || c == ' ').filter(|s| !s.is_empty()).collect();
    let parts2: Vec<&str> = name2.split(|c| c == ',' || c == ' ').filter(|s| !s.is_empty()).collect();

    if parts1.len() >= 2 && parts2.len() >= 2 {
        // Check if first and last names match (in any order)
        let set1: std::collections::HashSet<&str> = parts1.iter().copied().collect();
        let set2: std::collections::HashSet<&str> = parts2.iter().copied().collect();
        let intersection: Vec<&&str> = set1.intersection(&set2).collect();
        return intersection.len() >= 2;
    }

    false
}

/// Grade a single prediction against boxscore data
pub fn grade_prediction(
    prediction_id: &str,
    player_name: &str,
    pick_type: &str,
    line: f64,
    stat_category: &str,
    boxscore: &HashMap<String, HashMap<String, f64>>,
    game_id: &str,
    game_status: &str,
) -> GradingResult {
    let actual = lookup_player_stat(boxscore, player_name, stat_category);

    match actual {
        Some(actual_val) => {
            let outcome = match pick_type.to_lowercase().as_str() {
                "over" => {
                    if actual_val > line {
                        "Win"
                    } else if actual_val < line {
                        "Loss"
                    } else {
                        "Push"
                    }
                }
                "under" => {
                    if actual_val < line {
                        "Win"
                    } else if actual_val > line {
                        "Loss"
                    } else {
                        "Push"
                    }
                }
                _ => {
                    return GradingResult {
                        prediction_id: prediction_id.to_string(),
                        player_name: player_name.to_string(),
                        stat_category: stat_category.to_string(),
                        line,
                        pick_type: pick_type.to_string(),
                        outcome: "Unresolved".to_string(),
                        actual_result: Some(actual_val),
                        game_id: Some(game_id.to_string()),
                        game_status: Some(game_status.to_string()),
                        notes: format!(
                            "Unknown pick type '{}' — cannot grade. Actual stat: {}",
                            pick_type, actual_val
                        ),
                    };
                }
            };

            GradingResult {
                prediction_id: prediction_id.to_string(),
                player_name: player_name.to_string(),
                stat_category: stat_category.to_string(),
                line,
                pick_type: pick_type.to_string(),
                outcome: outcome.to_string(),
                actual_result: Some(actual_val),
                game_id: Some(game_id.to_string()),
                game_status: Some(game_status.to_string()),
                notes: format!(
                    "Actual: {} {} — Line: {} {} → {}",
                    player_name, actual_val, pick_type, line, outcome
                ),
            }
        }
        None => GradingResult {
            prediction_id: prediction_id.to_string(),
            player_name: player_name.to_string(),
            stat_category: stat_category.to_string(),
            line,
            pick_type: pick_type.to_string(),
            outcome: "Unresolved".to_string(),
            actual_result: None,
            game_id: Some(game_id.to_string()),
            game_status: Some(game_status.to_string()),
            notes: format!(
                "Could not find stat '{}' for player '{}' in game boxscore",
                stat_category, player_name
            ),
        },
    }
}

/// Main grading function: fetch completed games, get boxscores, grade all pending predictions
pub async fn grade_all_pending(
    pending: &[(String, String, String, f64, String, String)], // (id, player, pick_type, line, stat_cat, session_id)
) -> GradingSummary {
    let fetched_at = chrono::Utc::now().to_rfc3339();
    let total_pending = pending.len();

    // Fetch completed games
    let games = match fetch_completed_games().await {
        Ok(g) => g,
        Err(e) => {
            return GradingSummary {
                total_pending,
                graded: 0,
                skipped: total_pending,
                wins: 0,
                losses: 0,
                pushes: 0,
                unresolved: total_pending,
                results: pending
                    .iter()
                    .map(|(id, player, pick_type, line, stat_cat, _)| GradingResult {
                        prediction_id: id.clone(),
                        player_name: player.clone(),
                        stat_category: stat_cat.clone(),
                        line: *line,
                        pick_type: pick_type.clone(),
                        outcome: "Unresolved".to_string(),
                        actual_result: None,
                        game_id: None,
                        game_status: None,
                        notes: format!("Failed to fetch completed games: {}", e),
                    })
                    .collect(),
                fetched_at,
            };
        }
    };

    if games.is_empty() {
        return GradingSummary {
            total_pending,
            graded: 0,
            skipped: total_pending,
            wins: 0,
            losses: 0,
            pushes: 0,
            unresolved: total_pending,
            results: pending
                .iter()
                .map(|(id, player, pick_type, line, stat_cat, _)| GradingResult {
                    prediction_id: id.clone(),
                    player_name: player.clone(),
                    stat_category: stat_cat.clone(),
                    line: *line,
                    pick_type: pick_type.clone(),
                    outcome: "Unresolved".to_string(),
                    actual_result: None,
                    game_id: None,
                    game_status: Some("No completed games found".to_string()),
                    notes: "No completed games on the current scoreboard".to_string(),
                })
                .collect(),
            fetched_at,
        };
    }

    // Fetch boxscores for all completed games
    let mut all_boxscores: HashMap<String, HashMap<String, HashMap<String, f64>>> = HashMap::new();
    for game in &games {
        match fetch_game_boxscore(&game.game_id).await {
            Ok(boxscore) => {
                all_boxscores.insert(game.game_id.clone(), boxscore);
            }
            Err(e) => {
                tracing::warn!("Failed to fetch boxscore for game {}: {}", game.game_id, e);
            }
        }
    }

    // Grade each pending prediction
    let mut results = Vec::new();
    let mut graded = 0usize;
    let mut skipped = 0usize;
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut pushes = 0usize;
    let mut unresolved = 0usize;

    for (pred_id, player, pick_type, line, stat_cat, _) in pending {
        let mut best_result: Option<GradingResult> = None;

        // Try each game's boxscore
        for (game_id, boxscore) in &all_boxscores {
            let result = grade_prediction(
                pred_id,
                player,
                pick_type,
                *line,
                stat_cat,
                boxscore,
                game_id,
                "Final",
            );

            if result.outcome != "Unresolved" {
                best_result = Some(result);
                break;
            }

            // Keep the "not found" result as fallback
            if best_result.is_none() {
                best_result = Some(result);
            }
        }

        if let Some(result) = best_result {
            match result.outcome.as_str() {
                "Win" => {
                    graded += 1;
                    wins += 1;
                }
                "Loss" => {
                    graded += 1;
                    losses += 1;
                }
                "Push" => {
                    graded += 1;
                    pushes += 1;
                }
                _ => {
                    skipped += 1;
                    unresolved += 1;
                }
            }
            results.push(result);
        } else {
            // No boxscores available at all
            skipped += 1;
            unresolved += 1;
            results.push(GradingResult {
                prediction_id: pred_id.clone(),
                player_name: player.clone(),
                stat_category: stat_cat.clone(),
                line: *line,
                pick_type: pick_type.clone(),
                outcome: "Unresolved".to_string(),
                actual_result: None,
                game_id: None,
                game_status: None,
                notes: "No boxscore data available for any completed game".to_string(),
            });
        }
    }

    GradingSummary {
        total_pending,
        graded,
        skipped,
        wins,
        losses,
        pushes,
        unresolved,
        results,
        fetched_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_category() {
        assert_eq!(normalize_category("passing yards"), "Passing Yards");
        assert_eq!(normalize_category("Rushing Yards"), "Rushing Yards");
        assert_eq!(normalize_category("receptions"), "Receptions");
        assert_eq!(normalize_category("Passing Touchdowns"), "Passing Touchdowns");
    }

    #[test]
    fn test_grade_prediction_over_win() {
        let mut boxscore: HashMap<String, HashMap<String, f64>> = HashMap::new();
        let mut stats = HashMap::new();
        stats.insert("passingYards".to_string(), 300.0);
        boxscore.insert("Patrick Mahomes".to_string(), stats);

        let result = grade_prediction(
            "pred-1",
            "Patrick Mahomes",
            "Over",
            285.5,
            "Passing Yards",
            &boxscore,
            "game-123",
            "Final",
        );

        assert_eq!(result.outcome, "Win");
        assert_eq!(result.actual_result, Some(300.0));
    }

    #[test]
    fn test_grade_prediction_over_loss() {
        let mut boxscore: HashMap<String, HashMap<String, f64>> = HashMap::new();
        let mut stats = HashMap::new();
        stats.insert("passingYards".to_string(), 250.0);
        boxscore.insert("Patrick Mahomes".to_string(), stats);

        let result = grade_prediction(
            "pred-1",
            "Patrick Mahomes",
            "Over",
            285.5,
            "Passing Yards",
            &boxscore,
            "game-123",
            "Final",
        );

        assert_eq!(result.outcome, "Loss");
        assert_eq!(result.actual_result, Some(250.0));
    }

    #[test]
    fn test_grade_prediction_push() {
        let mut boxscore: HashMap<String, HashMap<String, f64>> = HashMap::new();
        let mut stats = HashMap::new();
        stats.insert("passingYards".to_string(), 285.5);
        boxscore.insert("Patrick Mahomes".to_string(), stats);

        let result = grade_prediction(
            "pred-1",
            "Patrick Mahomes",
            "Over",
            285.5,
            "Passing Yards",
            &boxscore,
            "game-123",
            "Final",
        );

        assert_eq!(result.outcome, "Push");
        assert_eq!(result.actual_result, Some(285.5));
    }

    #[test]
    fn test_grade_prediction_under_win() {
        let mut boxscore: HashMap<String, HashMap<String, f64>> = HashMap::new();
        let mut stats = HashMap::new();
        stats.insert("rushingYards".to_string(), 75.0);
        boxscore.insert("Saquon Barkley".to_string(), stats);

        let result = grade_prediction(
            "pred-2",
            "Saquon Barkley",
            "Under",
            88.5,
            "Rushing Yards",
            &boxscore,
            "game-456",
            "Final",
        );

        assert_eq!(result.outcome, "Win");
        assert_eq!(result.actual_result, Some(75.0));
    }

    #[test]
    fn test_grade_prediction_player_not_found() {
        let boxscore: HashMap<String, HashMap<String, f64>> = HashMap::new();

        let result = grade_prediction(
            "pred-3",
            "Unknown Player",
            "Over",
            100.0,
            "Yards",
            &boxscore,
            "game-789",
            "Final",
        );

        assert_eq!(result.outcome, "Unresolved");
        assert_eq!(result.actual_result, None);
    }

    #[test]
    fn test_fuzzy_name_matching() {
        assert!(name_matches_fuzzy("patrick mahomes", "mahomes, patrick"));
        assert!(name_matches_fuzzy("mahomes, patrick", "patrick mahomes"));
        assert!(!name_matches_fuzzy("patrick mahomes", "tom brady"));
    }

    #[test]
    fn test_lookup_player_stat_fuzzy() {
        let mut boxscore: HashMap<String, HashMap<String, f64>> = HashMap::new();
        let mut stats = HashMap::new();
        stats.insert("passingYards".to_string(), 300.0);
        boxscore.insert("Patrick Mahomes".to_string(), stats);

        // Should find via exact match
        let val = lookup_player_stat(&boxscore, "Patrick Mahomes", "Passing Yards");
        assert_eq!(val, Some(300.0));

        // Should find via partial match
        let val2 = lookup_player_stat(&boxscore, "Mahomes", "Passing Yards");
        assert_eq!(val2, Some(300.0));

        // Should not find non-existent player
        let val3 = lookup_player_stat(&boxscore, "Tom Brady", "Passing Yards");
        assert_eq!(val3, None);
    }
}
