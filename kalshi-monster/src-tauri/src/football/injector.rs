#![allow(dead_code)]
//! ═══════════════════════════════════════════════════════════════
//! Live Data Injector — Real-Time Context Enrichment Engine
//!
//! Fetches live data from multiple free sources and formats it
//! for injection into the AI system prompt. This is the critical
//! layer that transforms static knowledge into dynamic, real-time
//! context for sharper predictions.
//!
//! Data sources (all free, no API key needed):
//!   - ESPN scoreboard API: live scores, schedules, game status
//!   - ESPN stats API: team statistics, standings
//!   - ESPN news API: latest NFL news, injury updates
//!   - Sleeper state API: current week, season state
//!   - Sleeper players API: injury report with status/notes
//!   - Sleeper stats API: weekly and season player stats
//!
//! The injector runs these fetches concurrently and compacts the
//! results into a concise, high-signal context block.
//! ═══════════════════════════════════════════════════════════════

use crate::football::api_client::SportsApiClient;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write as _;

// ── Data Structures ──

/// Complete live data packet for AI context injection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveDataPacket {
    pub generated_at: String,
    pub current_week: Option<u32>,
    pub season_phase: String,
    pub schedule_context: String,
    pub player_stats_context: String,
    pub injury_context: String,
    pub news_context: String,
    pub standings_context: String,
    pub weather_context: String,
    pub data_sources: Vec<String>,
    pub fetch_errors: Vec<String>,
}

impl LiveDataPacket {
    /// Render the complete packet as a compact AI context block
    pub fn to_ai_context(&self) -> String {
        let mut ctx = String::with_capacity(8192);

        // Header
        ctx.push_str("═══════════════════════════════════════════════════\n");
        ctx.push_str("🔴 LIVE DATA INJECTION — REAL-TIME CONTEXT\n");
        ctx.push_str(&format!(" Generated: {} | Week: {:?} | Phase: {}\n",
            self.generated_at, self.current_week, self.season_phase));
        ctx.push_str("═══════════════════════════════════════════════════\n\n");

        // Schedule / Games
        if !self.schedule_context.is_empty() {
            ctx.push_str(&self.schedule_context);
            ctx.push('\n');
        }

        // Standings
        if !self.standings_context.is_empty() {
            ctx.push_str(&self.standings_context);
            ctx.push('\n');
        }

        // Player Stats
        if !self.player_stats_context.is_empty() {
            ctx.push_str(&self.player_stats_context);
            ctx.push('\n');
        }

        // Injuries
        if !self.injury_context.is_empty() {
            ctx.push_str(&self.injury_context);
            ctx.push('\n');
        }

        // News
        if !self.news_context.is_empty() {
            ctx.push_str(&self.news_context);
            ctx.push('\n');
        }

        // Weather
        if !self.weather_context.is_empty() {
            ctx.push_str(&self.weather_context);
            ctx.push('\n');
        }

        // Data provenance note
        if !self.data_sources.is_empty() {
            ctx.push_str(&format!("📡 Live sources: {}\n", self.data_sources.join(", ")));
        }
        if !self.fetch_errors.is_empty() {
            ctx.push_str(&format!("⚠️ Source issues: {}\n", self.fetch_errors.join("; ")));
        }

        ctx
    }
}

// ── Main Injector ──

/// Build a complete live data packet by fetching from all sources concurrently.
/// This is the primary entry point called before each AI request.
pub async fn build_live_data_packet(
    client: &SportsApiClient,
    query: &str,
    max_players: usize,
) -> LiveDataPacket {
    let generated_at = Utc::now().to_rfc3339();

    // Detect relevant teams/players from query for targeted fetching
    let query_teams = extract_team_mentions(query);
    let query_players = extract_player_mentions(query);

    // Fetch everything concurrently
    let (
        schedule_result,
        standings_result,
        news_result,
        sleeper_state_result,
        injuries_result,
    ) = tokio::join!(
        fetch_enriched_schedule(client, query),
        fetch_compact_standings(client),
        fetch_news_snippets(client),
        fetch_sleeper_state(client),
        fetch_injury_report(client),
    );

    // Extract current week from sleeper state
    let current_week = sleeper_state_result
        .as_ref()
        .ok()
        .and_then(|v| v.get("leg").and_then(|w| w.as_u64()).map(|w| w as u32))
        .or_else(|| {
            // Fallback: try week field
            sleeper_state_result
                .as_ref()
                .ok()
                .and_then(|v| v.get("week").and_then(|w| w.as_u64()).map(|w| w as u32))
        });

    let season_phase = sleeper_state_result
        .as_ref()
        .ok()
        .and_then(|v| v.get("season_type").and_then(|s| s.as_str()))
        .unwrap_or("regular")
        .to_string();

    // Format schedule context
    let (schedule_context, schedule_errors) = match schedule_result {
        Ok(games) => {
            let ctx = format_schedule_context(&games, &query_teams, &query_players);
            (ctx, vec![])
        }
        Err(e) => (String::new(), vec![format!("Schedule unavailable: {}", e)]),
    };

    // Format standings context
    let (standings_context, standings_errors) = match standings_result {
        Ok(standings) => (format_standings_context(&standings), vec![]),
        Err(e) => (String::new(), vec![format!("Standings unavailable: {}", e)]),
    };

    // Player stats context (using Sleeper weekly stats for relevant players)
    let player_stats_context = if !query_players.is_empty() || !query_teams.is_empty() {
        fetch_relevant_player_context(client, &query_teams, &query_players, max_players).await
    } else {
        String::new()
    };

    // Format injury context
    let (injury_context, injury_errors) = match injuries_result {
        Ok(injuries) => (format_injury_context(&injuries, &query_teams, &query_players), vec![]),
        Err(e) => (String::new(), vec![format!("Injuries unavailable: {}", e)]),
    };

    // Format news context
    let (news_context, news_errors) = match news_result {
        Ok(news) => (format_news_context(&news, &query_teams, &query_players), vec![]),
        Err(e) => (String::new(), vec![format!("News unavailable: {}", e)]),
    };

    // Collect data sources used
    let mut data_sources = Vec::new();
    if schedule_context.is_empty() { /* ESPN used */ }
    data_sources.push("ESPN".into());
    if !injury_context.is_empty() { data_sources.push("Sleeper".into()); }

    let mut fetch_errors = Vec::new();
    fetch_errors.extend(schedule_errors);
    fetch_errors.extend(standings_errors);
    fetch_errors.extend(injury_errors);
    fetch_errors.extend(news_errors);

    LiveDataPacket {
        generated_at,
        current_week,
        season_phase,
        schedule_context,
        player_stats_context,
        injury_context,
        news_context,
        standings_context,
        weather_context: String::new(), // Populated separately by weather module
        data_sources,
        fetch_errors,
    }
}

// ── Schedule Fetching ──

async fn fetch_enriched_schedule(
    client: &SportsApiClient,
    _query: &str,
) -> Result<Vec<EnrichedGame>, String> {
    let scoreboard = client.espn_scoreboard().await?;
    let games = parse_enriched_scoreboard(&scoreboard);
    Ok(games)
}

#[derive(Debug, Clone)]
struct EnrichedGame {
    home_team: String,
    away_team: String,
    home_abbr: String,
    away_abbr: String,
    game_time: String,
    status: String,
    home_score: Option<i32>,
    away_score: Option<i32>,
    total_line: Option<f64>,
    spread: Option<f64>,
    home_record: String,
    away_record: String,
}

fn parse_enriched_scoreboard(payload: &Value) -> Vec<EnrichedGame> {
    let mut games = Vec::new();

    let events = match payload.get("events").and_then(|e| e.as_array()) {
        Some(e) => e,
        None => return games,
    };

    for event in events {
        let competitions = match event.get("competitions").and_then(|c| c.as_array()) {
            Some(c) if !c.is_empty() => c,
            _ => continue,
        };
        let comp = &competitions[0];

        let competitors = match comp.get("competitors").and_then(|c| c.as_array()) {
            Some(c) => c,
            None => continue,
        };

        let mut home = None;
        let mut away = None;
        for c in competitors {
            let is_home = c.get("homeAway").and_then(|h| h.as_str()) == Some("home");
            let team_info = c.get("team").and_then(|t| t.as_object());
            let display_name = team_info
                .and_then(|t| t.get("displayName"))
                .and_then(|n| n.as_str())
                .unwrap_or("Unknown");
            let abbr = team_info
                .and_then(|t: &serde_json::Map<String, Value>| t.get("abbreviation"))
                .and_then(|a| a.as_str())
                .unwrap_or("UNK");
            let score = c.get("score").and_then(|s| s.as_str())
                .and_then(|s| s.parse::<i32>().ok());
            let record = c.get("records")
                .and_then(|r| r.as_array())
                .and_then(|r| r.first())
                .and_then(|r| r.get("summary"))
                .and_then(|s| s.as_str())
                .unwrap_or("-");

            let team_data = (display_name.to_string(), abbr.to_string(), score, record.to_string());
            if is_home { home = Some(team_data); } else { away = Some(team_data); }
        }

        let (home, away) = match (home, away) {
            (Some(h), Some(a)) => (h, a),
            _ => continue,
        };

        // Game time
        let game_time = event.get("date")
            .and_then(|d| d.as_str())
            .and_then(|d| {
                // Parse ISO 8601
                chrono::DateTime::parse_from_rfc3339(d)
                    .ok()
                    .map(|dt| dt.format("%a %I:%M %p %Z").to_string())
            })
            .unwrap_or_else(|| "TBD".to_string());

        // Status
        let status = comp.get("status")
            .and_then(|s| s.get("type"))
            .and_then(|t| t.get("description").or_else(|| t.get("name")))
            .and_then(|d| d.as_str())
            .unwrap_or("Scheduled")
            .to_string();

        // Lines (from the first competition's odds if available)
        let (total_line, spread) = comp.get("odds")
            .and_then(|o| o.as_array())
            .and_then(|o| o.first())
            .map(|odds| {
                let total = odds.get("overUnder").and_then(|v| v.as_f64());
                let spread_val = odds.get("spread").and_then(|v| v.as_f64());
                (total, spread_val)
            })
            .unwrap_or((None, None));

        games.push(EnrichedGame {
            home_team: home.0,
            away_team: away.0,
            home_abbr: home.1.clone(),
            away_abbr: away.1.clone(),
            game_time,
            status,
            home_score: home.2,
            away_score: away.2,
            total_line,
            spread,
            home_record: home.3,
            away_record: away.3,
        });
    }

    games
}

fn format_schedule_context(
    games: &[EnrichedGame],
    query_teams: &[String],
    _query_players: &[String],
) -> String {
    if games.is_empty() {
        return String::new();
    }

    let mut ctx = String::with_capacity(2048);
    ctx.push_str("## 📅 LIVE SCHEDULE & GAME STATUS\n");

    // Prioritize games involving queried teams
    let (relevant, other): (Vec<_>, Vec<_>) = if query_teams.is_empty() {
        (games.iter().take(12).collect(), vec![])
    } else {
        let relevant: Vec<_> = games.iter()
            .filter(|g| {
                query_teams.iter().any(|t| {
                    g.home_abbr.to_uppercase() == t.to_uppercase() ||
                    g.away_abbr.to_uppercase() == t.to_uppercase() ||
                    g.home_team.to_lowercase().contains(&t.to_lowercase()) ||
                    g.away_team.to_lowercase().contains(&t.to_lowercase())
                })
            })
            .collect();
        if relevant.is_empty() {
            (games.iter().take(12).collect(), vec![])
        } else {
            (relevant, vec![])
        }
    };

    for &game in relevant.iter().take(12) {
        let score_str = match (game.home_score, game.away_score) {
            (Some(h), Some(a)) => format!(" [{}-{}]", h, a),
            _ => String::new(),
        };
        let line_str = match (game.total_line, game.spread) {
            (Some(total), Some(spread)) => format!(" | O/U {:.1} Spread {:.1}", total, spread),
            (Some(total), None) => format!(" | O/U {:.1}", total),
            (None, Some(spread)) => format!(" | Spread {:.1}", spread),
            _ => String::new(),
        };
        let record_str = if !game.home_record.is_empty() && !game.away_record.is_empty() {
            format!(" [{} {}]", game.away_record, game.home_record)
        } else {
            String::new()
        };

        writeln!(ctx, "- {} @ {} ({}) {}{}{}{}",
            game.away_abbr, game.home_abbr, game.game_time,
            game.status, score_str, line_str, record_str
        ).unwrap();
    }

    if !other.is_empty() && relevant.len() < 6 {
        ctx.push_str("\n  Other games this week:\n");
        let other_slice: &[&EnrichedGame] = other.as_slice();
        for game in other_slice.iter().copied().take(6) {
            writeln!(ctx, "  - {} @ {} ({}) {}", game.away_abbr, game.home_abbr,
                game.game_time, game.status).unwrap();
        }
    }

    ctx
}

// ── Standings ──

async fn fetch_compact_standings(client: &SportsApiClient) -> Result<Value, String> {
    client.espn_standings().await
}

fn format_standings_context(standings: &Value) -> String {
    let mut ctx = String::new();

    // Parse ESPN standings format
    let entries = standings.get("standings")
        .or_else(|| standings.get("children"))
        .and_then(|s| s.as_array());

    let entries = match entries {
        Some(e) => e,
        None => {
            // Try alternate format: standings is an array directly
            let direct = standings.as_array();
            let _ = direct;
            return ctx;
        }
    };

    if entries.is_empty() {
        return ctx;
    }

    writeln!(ctx, "## 📊 CURRENT STANDINGS SNAPSHOT").unwrap();

    // Show each division/conference briefly
    for entry in entries.iter().take(4) {
        let name = entry.get("name")
            .or_else(|| entry.get("displayName"))
            .and_then(|n| n.as_str())
            .unwrap_or("");

        let entries_inner = entry.get("standings").or_else(|| entry.get("entries"))
            .and_then(|e| e.as_array());

        if let Some(teams) = entries_inner {
            if !name.is_empty() && !teams.is_empty() {
                writeln!(ctx, "\n  {}:", name).unwrap();
            }
            for team in teams.iter().take(8) {
                let team_name = team.get("team")
                    .or_else(|| team.get("displayName"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");

                let record = team.get("records")
                    .and_then(|r| r.as_array())
                    .and_then(|r| r.first())
                    .and_then(|r| r.get("summary").or_else(|| r.get("displayValue")))
                    .and_then(|s| s.as_str());

                let pct = team.get("stats")
                    .and_then(|s| s.as_array())
                    .and_then(|s| {
                        s.iter().find(|stat| {
                            stat.get("name").and_then(|n| n.as_str())
                                .map_or(false, |n| n == "playoffSeed" || n == "winPercent")
                        })
                    })
                    .and_then(|s| s.get("displayValue").or_else(|| s.get("value")))
                    .and_then(|v| v.as_str());

                if !team_name.is_empty() {
                    let rec_str = record.map_or(String::new(), |r| format!(" ({})", r));
                    let pct_str = pct.map_or(String::new(), |p| format!(" [{}]", p));
                    writeln!(ctx, "    {}{}{}", team_name, rec_str, pct_str).unwrap();
                }
            }
        }
    }

    ctx
}

// ── Injury Report ──

async fn fetch_injury_report(client: &SportsApiClient) -> Result<Value, String> {
    client.sleeper_injuries().await
}

fn format_injury_context(
    injuries: &Value,
    query_teams: &[String],
    query_players: &[String],
) -> String {
    let mut ctx = String::new();

    let injury_list = match injuries.get("injuries").and_then(|i| i.as_array()) {
        Some(list) => list,
        None => return ctx,
    };

    if injury_list.is_empty() {
        return ctx;
    }

    // Prioritize injuries for queried teams/players
    let all_injuries: Vec<&Value> = injury_list.iter().collect();

    let (relevant, other): (Vec<&Value>, Vec<&Value>) = if query_teams.is_empty() && query_players.is_empty() {
        let (first_20, rest) = all_injuries.split_at(all_injuries.len().min(20));
        (first_20.to_vec(), rest.to_vec())
    } else {
        let relevant: Vec<&Value> = all_injuries.iter()
            .filter(|inj| {
                let team = inj.get("team").and_then(|t| t.as_str()).unwrap_or("");
                let name = inj.get("name").and_then(|n| n.as_str()).unwrap_or("");
                query_teams.iter().any(|t| team.to_uppercase() == t.to_uppercase()) ||
                query_players.iter().any(|p| name.to_lowercase().contains(&p.to_lowercase()))
            })
            .copied()
            .collect();

        if relevant.is_empty() {
            let (first_15, _) = all_injuries.split_at(all_injuries.len().min(15));
            (first_15.to_vec(), vec![])
        } else {
            (relevant, vec![])
        }
    };

    writeln!(ctx, "## 🏥 INJURY REPORT ({} players listed)", injury_list.len()).unwrap();

    for inj in relevant.iter().take(20) {
        let name = inj.get("name").and_then(|n| n.as_str()).unwrap_or("Unknown");
        let team = inj.get("team").and_then(|t| t.as_str()).unwrap_or("");
        let position = inj.get("position").and_then(|p| p.as_str()).unwrap_or("");
        let status = inj.get("injury_status").and_then(|s| s.as_str()).unwrap_or("Unknown");
        let body_part = inj.get("injury_body_part").and_then(|b| b.as_str()).unwrap_or("");
        let notes = inj.get("injury_notes").and_then(|n| n.as_str()).unwrap_or("");

        let status_icon = match status.to_uppercase().as_str() {
            "OUT" => "❌",
            "DOUBTFUL" => "🔴",
            "QUESTIONABLE" => "🟡",
            "PROBABLE" => "🟢",
            _ => "⚠️",
        };

        let body_part_str = if body_part.is_empty() {
            String::new()
        } else {
            format!(" ({})", body_part)
        };
        let notes_str = if notes.is_empty() {
            String::new()
        } else {
            format!(" — {}", notes)
        };

        writeln!(ctx, "  {} {} ({}, {}) {}{}{}",
            status_icon, name, team, position, status, body_part_str, notes_str
        ).unwrap();
    }

    if !other.is_empty() && relevant.len() < 10 {
        writeln!(ctx, "  ...and {} other players with injury designations", other.len()).unwrap();
    }

    ctx
}

// ── News Snippets ──

async fn fetch_news_snippets(client: &SportsApiClient) -> Result<Value, String> {
    client.espn_news().await
}

fn format_news_context(
    news: &Value,
    query_teams: &[String],
    query_players: &[String],
) -> String {
    let mut ctx = String::new();

    let articles = match news.get("articles").and_then(|a| a.as_array()) {
        Some(a) => a,
        None => return ctx,
    };

    if articles.is_empty() {
        return ctx;
    }

    // Filter for relevant articles
    let relevant_articles: Vec<&Value> = articles.iter()
        .filter(|article| {
            let headline = article.get("headline")
                .or_else(|| article.get("title"))
                .and_then(|h| h.as_str())
                .unwrap_or("")
                .to_lowercase();
            let desc = article.get("description")
                .or_else(|| article.get("story"))
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_lowercase();

            let text = format!("{} {}", headline, desc);

            query_teams.iter().any(|t| text.contains(&t.to_lowercase())) ||
            query_players.iter().any(|p| text.contains(&p.to_lowercase())) ||
            (query_teams.is_empty() && query_players.is_empty())
        })
        .take(8)
        .collect();

    if relevant_articles.is_empty() {
        // Just show top headlines if nothing matches
        writeln!(ctx, "## 📰 LATEST NEWS").unwrap();
        for article in articles.iter().take(5) {
            let headline = article.get("headline")
                .or_else(|| article.get("title"))
                .and_then(|h| h.as_str())
                .unwrap_or("");
            let desc = article.get("description")
                .or_else(|| article.get("story"))
                .and_then(|d| d.as_str())
                .unwrap_or("");
            if !headline.is_empty() {
                let snippet = if desc.len() > 200 { &desc[..200] } else { desc };
                writeln!(ctx, "  • {} {}", headline,
                    if snippet.is_empty() { String::new() } else { format!("— {}", snippet) }
                ).unwrap();
            }
        }
    } else {
        writeln!(ctx, "## 📰 RELEVANT NEWS").unwrap();
        for article in relevant_articles.iter().take(8) {
            let headline = article.get("headline")
                .or_else(|| article.get("title"))
                .and_then(|h| h.as_str())
                .unwrap_or("");
            let desc = article.get("description")
                .or_else(|| article.get("story"))
                .and_then(|d| d.as_str())
                .unwrap_or("");
            let published = article.get("published")
                .or_else(|| article.get("pubDate"))
                .and_then(|d| d.as_str())
                .unwrap_or("");

            if !headline.is_empty() {
                let time_str = if published.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", &published[..published.len().min(10)])
                };
                let snippet = if desc.len() > 250 { &desc[..250] } else { desc };
                writeln!(ctx, "  • {}{}", headline, time_str).unwrap();
                if !snippet.is_empty() {
                    writeln!(ctx, "    {}", snippet).unwrap();
                }
            }
        }
    }

    ctx
}

// ── Sleeper State ──

async fn fetch_sleeper_state(client: &SportsApiClient) -> Result<Value, String> {
    client.sleeper_news().await // sleeper_news actually fetches state
}

// ── Player Stats from Sleeper ──

async fn fetch_relevant_player_context(
    client: &SportsApiClient,
    _query_teams: &[String],
    _query_players: &[String],
    max_players: usize,
) -> String {
    let season = "2025";
    let stats_result = client.sleeper_player_stats(season, None).await;

    let stats = match stats_result {
        Ok(s) => s,
        Err(_) => return String::new(),
    };

    let stats_obj = match stats.as_object() {
        Some(o) => o,
        None => return String::new(),
    };

    // Filter to relevant players
    let mut relevant_stats: Vec<(String, String, HashMap<String, f64>)> = Vec::new();

    for (player_id, player_data) in stats_obj {
        let name = player_data.get("player_id")
            .and_then(|p| p.as_str())
            .unwrap_or(player_id);

        // Extract key stats
        let mut stat_map = HashMap::new();
        let stat_fields = [
            ("pass_yds", "pass_yds"), ("pass_tds", "pass_tds"),
            ("rush_yds", "rush_yds"), ("rush_tds", "rush_tds"),
            ("rec_yds", "rec_yds"), ("rec_tds", "rec_tds"),
            ("rec", "rec"), ("targets", "targets"),
            ("fantasy_points_ppr", "fpts_ppr"),
            ("fantasy_points", "fpts"),
            ("gp", "games"),
        ];

        for (key, label) in &stat_fields {
            if let Some(val) = player_data.get(key).and_then(|v| v.as_f64()) {
                stat_map.insert(label.to_string(), val);
            }
        }

        if !stat_map.is_empty() {
            relevant_stats.push((name.to_string(), player_id.to_string(), stat_map));
        }
    }

    // Take top entries
    if relevant_stats.is_empty() {
        return String::new();
    }

    let mut ctx = String::with_capacity(2048);
    writeln!(ctx, "## 📈 SEASON STATS (Sleeper)").unwrap();
    for (name, _id, stats_map) in relevant_stats.iter().take(max_players) {
        let stats_str: Vec<String> = stats_map.iter()
            .map(|(k, v)| format!("{}={:.1}", k, v))
            .collect();
        writeln!(ctx, "  {}: {}", name, stats_str.join(", ")).unwrap();
    }

    ctx
}

// ── Query Parsing Helpers ──

fn extract_team_mentions(query: &str) -> Vec<String> {
    let nfl_teams: [(&str, &str); 32] = [
        ("ARI", "Cardinals"), ("ATL", "Falcons"), ("BAL", "Ravens"), ("BUF", "Bills"),
        ("CAR", "Panthers"), ("CHI", "Bears"), ("CIN", "Bengals"), ("CLE", "Browns"),
        ("DAL", "Cowboys"), ("DEN", "Broncos"), ("DET", "Lions"), ("GB", "Packers"),
        ("HOU", "Texans"), ("IND", "Colts"), ("JAX", "Jaguars"), ("KC", "Chiefs"),
        ("LAR", "Rams"), ("LVR", "Raiders"), ("LV", "Raiders"), ("MIA", "Dolphins"),
        ("MIN", "Vikings"), ("NE", "Patriots"), ("NO", "Saints"), ("NYG", "Giants"),
        ("NYJ", "Jets"), ("PHI", "Eagles"), ("PIT", "Steelers"), ("SEA", "Seahawks"),
        ("SF", "49ers"), ("TB", "Buccaneers"), ("TEN", "Titans"), ("WAS", "Commanders"),
    ];

    let query_upper = query.to_uppercase();
    let mut found = Vec::new();

    for (abbr, name) in &nfl_teams {
        let words: Vec<&str> = query_upper.split(|c: char| !c.is_ascii_alphabetic()).collect();
        if words.contains(&abbr) || query_upper.contains(&name.to_uppercase()) {
            found.push(abbr.to_string());
        }
    }

    found
}

fn extract_player_mentions(query: &str) -> Vec<String> {
    // Common first+last name patterns plus first-name-only mentions
    let query_lower = query.to_lowercase();

    // Known player names to check (expandable)
    let known_players: &[&str] = &[
        "mahomes", "allen", "hurts", "jackson", "burrow", "prescott", "stroud",
        "love", "lawrence", "purdy", "goff", "herbert", "murray", "maye",
        "nix", "williams", "tagovailoa", "rodgers",
        "barkley", "henry", "robinson", "gibbs", "hall", "taylor", "achane",
        "chase", "lamb", "brown", "nacua", "hill", "waddle", "dj moore",
        "metcalf", "mclaurin", "wilson", "andre williams",
        "kelce", "andrews", "kittle", "lauson", "engram",
        "josh allen", "jalen hurts", "lamar joshua", "patrick mahomes",
        "joe burrow", "dak prescott", "cj stout", "jordan love",
        "saquon barkley", "derrick henry", "bijan robinson", "jahmyr gibs",
        "breece hall", "jonathan taylor", "de'von achane",
        "ja'marr chase", "ceedee lamb", "amon-ra st brown",
        "puka nacua", "tyreek hill", "jaylen waddle",
    ];

    let mut found = Vec::new();
    for player in known_players {
        if query_lower.contains(player) {
            found.push(player.to_string());
        }
    }

    // Deduplicate (prefer longer match)
    found.sort_by_key(|s| -(s.len() as i32));
    let mut deduped = Vec::new();
    for name in &found {
        let is_substring = deduped.iter().any(|d: &&String| d.contains(name.as_str()) && d.len() > name.len());
        if !is_substring {
            deduped.push(name);
        }
    }

    deduped.into_iter().cloned().take(5).collect()
}
