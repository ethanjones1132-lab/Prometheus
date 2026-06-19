#![allow(dead_code)]
#![allow(async_fn_in_trait)]
//! ═══════════════════════════════════════════════════════════════
//! Sports Data API Client — Multi-Source Live Data Ingestion
//!
//! Primary sources (free, no API key):
//!   - ESPN site API: scores, schedules, team stats, player stats
//!   - Sleeper API: player news, injuries, fantasy stats
//!
//! Optional sources (API key required):
//!   - API-Sports (api-sports.io): structured NFL data, 100 req/day free
//!
//! Caching strategy: All responses are cached in memory with TTL to
//! minimize API calls and stay within free tier rate limits.
//! ═══════════════════════════════════════════════════════════════

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

// ── Cache ──

#[derive(Clone)]
struct CacheEntry {
    data: String,
    cached_at: Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn is_fresh(&self) -> bool {
        self.cached_at.elapsed() < self.ttl
    }
}

#[derive(Clone)]
pub struct ApiCache {
    entries: Arc<Mutex<HashMap<String, CacheEntry>>>,
}

impl Default for ApiCache {
    fn default() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl ApiCache {
    pub async fn get(&self, key: &str) -> Option<String> {
        let entries = self.entries.lock().await;
        entries.get(key).filter(|e| e.is_fresh()).map(|e| e.data.clone())
    }

    pub async fn set(&self, key: &str, data: String, ttl: Duration) {
        let mut entries = self.entries.lock().await;
        entries.insert(
            key.to_string(),
            CacheEntry {
                data,
                cached_at: Instant::now(),
                ttl,
            },
        );
    }

    pub async fn invalidate(&self, key: &str) {
        let mut entries = self.entries.lock().await;
        entries.remove(key);
    }

    pub async fn clear(&self) {
        let mut entries = self.entries.lock().await;
        entries.clear();
    }
}

// ── API Client Config ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SportsApiConfig {
    pub espn_base: String,
    pub sleeper_base: String,
    pub api_sports_key: String,
    pub api_sports_base: String,
    pub cache_ttl_secs: u64,
    pub request_timeout_secs: u64,
}

impl Default for SportsApiConfig {
    fn default() -> Self {
        Self {
            espn_base: "https://site.api.espn.com/apis/site/v2/sports/football/nfl".into(),
            sleeper_base: "https://api.sleeper.app/v1".into(),
            api_sports_key: String::new(),
            api_sports_base: "https://v1.american-football.api-sports.io".into(),
            cache_ttl_secs: 120, // 2 minutes default
            request_timeout_secs: 10,
        }
    }
}

// ── API Client ──

pub struct SportsApiClient {
    client: Client,
    config: SportsApiConfig,
    cache: ApiCache,
}

impl SportsApiClient {
    pub fn new(config: SportsApiConfig) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
        Ok(Self {
            client,
            config,
            cache: ApiCache::default(),
        })
    }

    pub fn cache(&self) -> &ApiCache {
        &self.cache
    }

    // ── ESPN: Scoreboard ──

    pub async fn espn_scoreboard(&self) -> Result<Value, String> {
        let cache_key = "espn_scoreboard";
        if let Some(cached) = self.cache.get(cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!("{}/scoreboard", self.config.espn_base);
        let resp = self
            .client
            .get(&url)
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

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(cache_key, json_str, Duration::from_secs(self.config.cache_ttl_secs))
            .await;

        Ok(json)
    }

    // ── ESPN: Team Stats ──

    pub async fn espn_team_stats(&self, team_id: &str) -> Result<Value, String> {
        let cache_key = format!("espn_team_stats_{}", team_id);
        if let Some(cached) = self.cache.get(&cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!("{}/teams/{}/statistics", self.config.espn_base, team_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("ESPN team stats request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("ESPN team stats returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("ESPN team stats parse error: {}", e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(&cache_key, json_str, Duration::from_secs(self.config.cache_ttl_secs * 5))
            .await;

        Ok(json)
    }

    // ── ESPN: Player Stats (via athlete endpoint) ──

    pub async fn espn_player_stats(&self, athlete_id: &str) -> Result<Value, String> {
        let cache_key = format!("espn_player_stats_{}", athlete_id);
        if let Some(cached) = self.cache.get(&cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!(
            "https://sports.core.api.espn.com/v2/sports/football/leagues/nfl/seasons/2025/athletes/{}/statistics",
            athlete_id
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("ESPN player stats request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("ESPN player stats returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("ESPN player stats parse error: {}", e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(&cache_key, json_str, Duration::from_secs(self.config.cache_ttl_secs * 5))
            .await;

        Ok(json)
    }

    // ── ESPN: Team Roster ──

    pub async fn espn_team_roster(&self, team_id: &str) -> Result<Value, String> {
        let cache_key = format!("espn_team_roster_{}", team_id);
        if let Some(cached) = self.cache.get(&cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!("{}/teams/{}/roster", self.config.espn_base, team_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("ESPN team roster request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("ESPN team roster returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("ESPN team roster parse error: {}", e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(
                &cache_key,
                json_str,
                Duration::from_secs(self.config.cache_ttl_secs * 30),
            )
            .await;

        Ok(json)
    }

    // ── ESPN: Game Summary (boxscore) ──

    pub async fn espn_game_summary(&self, event_id: &str) -> Result<Value, String> {
        let cache_key = format!("espn_game_summary_{}", event_id);
        if let Some(cached) = self.cache.get(&cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!("{}/summary?event={}", self.config.espn_base, event_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("ESPN game summary request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("ESPN game summary returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("ESPN game summary parse error: {}", e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(&cache_key, json_str, Duration::from_secs(self.config.cache_ttl_secs))
            .await;

        Ok(json)
    }

    // ── ESPN: Standings ──

    pub async fn espn_standings(&self) -> Result<Value, String> {
        let cache_key = "espn_standings";
        if let Some(cached) = self.cache.get(cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!("{}/standings", self.config.espn_base);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("ESPN standings request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("ESPN standings returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("ESPN standings parse error: {}", e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(
                cache_key,
                json_str,
                Duration::from_secs(self.config.cache_ttl_secs * 10),
            )
            .await;

        Ok(json)
    }

    // ── ESPN: News ──

    pub async fn espn_news(&self) -> Result<Value, String> {
        let cache_key = "espn_news";
        if let Some(cached) = self.cache.get(cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!("{}/news", self.config.espn_base);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("ESPN news request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("ESPN news returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("ESPN news parse error: {}", e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(
                cache_key,
                json_str,
                Duration::from_secs(self.config.cache_ttl_secs * 5),
            )
            .await;

        Ok(json)
    }

    // ── Sleeper: Player News / Injuries ──

    pub async fn sleeper_news(&self) -> Result<Value, String> {
        let cache_key = "sleeper_news";
        if let Some(cached) = self.cache.get(cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        // Sleeper doesn't have a direct news endpoint, but we can get player stats
        // which include injury status. We'll fetch the NFL state first.
        let url = format!("{}/state/nfl", self.config.sleeper_base);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Sleeper state request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Sleeper state returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("Sleeper state parse error: {}", e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(
                cache_key,
                json_str,
                Duration::from_secs(self.config.cache_ttl_secs * 5),
            )
            .await;

        Ok(json)
    }

    // ── Sleeper: Player Stats (season) ──

    pub async fn sleeper_player_stats(
        &self,
        season: &str,
        week: Option<u32>,
    ) -> Result<Value, String> {
        let cache_key = format!("sleeper_stats_{}_{:?}", season, week);
        if let Some(cached) = self.cache.get(&cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let week_str = week.map(|w| format!("/week/{}", w)).unwrap_or_default();
        let url = format!(
            "{}/stats/nfl/regular/{}{}",
            self.config.sleeper_base, season, week_str
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Sleeper stats request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Sleeper stats returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("Sleeper stats parse error: {}", e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(
                &cache_key,
                json_str,
                Duration::from_secs(self.config.cache_ttl_secs * 5),
            )
            .await;

        Ok(json)
    }

    // ── Sleeper: Injuries ──
    // Sleeper doesn't have a dedicated injuries endpoint.
    // We fetch the full NFL players list and filter for injury status.

    pub async fn sleeper_injuries(&self) -> Result<Value, String> {
        let cache_key = "sleeper_injuries";
        if let Some(cached) = self.cache.get(cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!("{}/players/nfl", self.config.sleeper_base);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Sleeper players request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Sleeper players returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("Sleeper players parse error: {}", e))?;

        // Filter to only injured players and restructure as injury report
        let injured_players = filter_injured_players(&json);

        let result = serde_json::json!({
            "source": "Sleeper",
            "fetched_at": chrono::Utc::now().to_rfc3339(),
            "injuries": injured_players,
        });

        let json_str = serde_json::to_string(&result).unwrap_or_default();
        self.cache
            .set(
                cache_key,
                json_str,
                Duration::from_secs(self.config.cache_ttl_secs * 3),
            )
            .await;

        Ok(result)
    }

    // ── API-Sports (optional, requires key) ──

    pub async fn api_sports_standings(&self) -> Result<Value, String> {
        if self.config.api_sports_key.is_empty() {
            return Err("API-Sports key not configured".into());
        }

        let cache_key = "api_sports_standings";
        if let Some(cached) = self.cache.get(cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!(
            "{}/standings?league=1&season=2025",
            self.config.api_sports_base
        );
        let resp = self
            .client
            .get(&url)
            .header("x-apisports-key", &self.config.api_sports_key)
            .send()
            .await
            .map_err(|e| format!("API-Sports standings request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("API-Sports standings returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("API-Sports standings parse error: {}", e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(
                cache_key,
                json_str,
                Duration::from_secs(self.config.cache_ttl_secs * 10),
            )
            .await;

        Ok(json)
    }

    pub async fn api_sports_teams(&self) -> Result<Value, String> {
        if self.config.api_sports_key.is_empty() {
            return Err("API-Sports key not configured".into());
        }

        let cache_key = "api_sports_teams";
        if let Some(cached) = self.cache.get(cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!("{}/teams?league=1&season=2025", self.config.api_sports_base);
        let resp = self
            .client
            .get(&url)
            .header("x-apisports-key", &self.config.api_sports_key)
            .send()
            .await
            .map_err(|e| format!("API-Sports teams request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("API-Sports teams returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("API-Sports teams parse error: {}", e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(
                cache_key,
                json_str,
                Duration::from_secs(self.config.cache_ttl_secs * 30),
            )
            .await;

        Ok(json)
    }

    pub async fn api_sports_games(&self, date: Option<&str>) -> Result<Value, String> {
        if self.config.api_sports_key.is_empty() {
            return Err("API-Sports key not configured".into());
        }

        let date_str = date.unwrap_or("2025-01-01");
        let cache_key = format!("api_sports_games_{}", date_str);
        if let Some(cached) = self.cache.get(&cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!(
            "{}/games?league=1&season=2025&date={}",
            self.config.api_sports_base, date_str
        );
        let resp = self
            .client
            .get(&url)
            .header("x-apisports-key", &self.config.api_sports_key)
            .send()
            .await
            .map_err(|e| format!("API-Sports games request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("API-Sports games returned HTTP {}", resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("API-Sports games parse error: {}", e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(&cache_key, json_str, Duration::from_secs(self.config.cache_ttl_secs))
            .await;

        Ok(json)
    }

    // ── ESPN: Multi-Sport Player Stats ──
    // Uses the ESPN core API which supports all sports.
    // URL pattern: https://sports.core.api.espn.com/v2/sports/{sport}/leagues/{league}/seasons/{year}/athletes/{id}/statistics

    /// Fetch player statistics for any sport league.
    /// `sport_path` is e.g. "basketball/nba", "baseball/mlb", "hockey/nhl", "football/nfl"
    pub async fn espn_sport_player_stats(
        &self,
        sport_path: &str,
        athlete_id: &str,
        season: u32,
    ) -> Result<Value, String> {
        let cache_key = format!("espn_{}_player_stats_{}_{}", sport_path.replace('/', "_"), athlete_id, season);
        if let Some(cached) = self.cache.get(&cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!(
            "https://sports.core.api.espn.com/v2/sports/{}/seasons/{}/athletes/{}/statistics",
            sport_path, season, athlete_id
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("ESPN {} player stats request failed: {}", sport_path, e))?;

        if !resp.status().is_success() {
            return Err(format!("ESPN {} player stats returned HTTP {}", sport_path, resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("ESPN {} player stats parse error: {}", sport_path, e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(&cache_key, json_str, Duration::from_secs(self.config.cache_ttl_secs * 5))
            .await;

        Ok(json)
    }

    /// Fetch team roster for any sport league.
    /// `sport_path` is e.g. "basketball/nba", "baseball/mlb", "hockey/nhl", "football/nfl"
    pub async fn espn_sport_team_roster(
        &self,
        sport_path: &str,
        team_id: &str,
        season: u32,
    ) -> Result<Value, String> {
        let cache_key = format!("espn_{}_team_roster_{}_{}", sport_path.replace('/', "_"), team_id, season);
        if let Some(cached) = self.cache.get(&cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!(
            "https://sports.core.api.espn.com/v2/sports/{}/seasons/{}/teams/{}/athletes",
            sport_path, season, team_id
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("ESPN {} team roster request failed: {}", sport_path, e))?;

        if !resp.status().is_success() {
            return Err(format!("ESPN {} team roster returned HTTP {}", sport_path, resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("ESPN {} team roster parse error: {}", sport_path, e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(&cache_key, json_str, Duration::from_secs(self.config.cache_ttl_secs * 30))
            .await;

        Ok(json)
    }

    /// Fetch season leaders (top players by stat category) for any sport.
    /// `sport_path` is e.g. "basketball/nba"
    pub async fn espn_sport_season_leaders(
        &self,
        sport_path: &str,
        season: u32,
    ) -> Result<Value, String> {
        let cache_key = format!("espn_{}_season_leaders_{}", sport_path.replace('/', "_"), season);
        if let Some(cached) = self.cache.get(&cache_key).await {
            return serde_json::from_str(&cached)
                .map_err(|e| format!("Cache parse error: {}", e));
        }

        let url = format!(
            "https://sports.core.api.espn.com/v2/sports/{}/seasons/{}/types/2/leaders",
            sport_path, season
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("ESPN {} season leaders request failed: {}", sport_path, e))?;

        if !resp.status().is_success() {
            return Err(format!("ESPN {} season leaders returned HTTP {}", sport_path, resp.status()));
        }

        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("ESPN {} season leaders parse error: {}", sport_path, e))?;

        let json_str = serde_json::to_string(&json).unwrap_or_default();
        self.cache
            .set(&cache_key, json_str, Duration::from_secs(self.config.cache_ttl_secs * 10))
            .await;

        Ok(json)
    }

    // ── Health check ──

    pub async fn check_espn_available(&self) -> bool {
        self.client
            .get(format!("{}/scoreboard", self.config.espn_base))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub async fn check_sleeper_available(&self) -> bool {
        self.client
            .get(format!("{}/state/nfl", self.config.sleeper_base))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub async fn check_api_sports_available(&self) -> bool {
        if self.config.api_sports_key.is_empty() {
            return false;
        }
        self.client
            .get(format!("{}/status", self.config.api_sports_base))
            .header("x-apisports-key", &self.config.api_sports_key)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

// ── Data Source Status ──

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DataSourceStatus {
    pub name: String,
    pub available: bool,
    pub requires_key: bool,
    pub key_configured: bool,
    pub description: String,
}

pub async fn check_all_sources(config: &SportsApiConfig) -> Vec<DataSourceStatus> {
    let client = match SportsApiClient::new(config.clone()) {
        Ok(c) => c,
        Err(_) => {
            return vec![
                DataSourceStatus {
                    name: "ESPN".into(),
                    available: false,
                    requires_key: false,
                    key_configured: true,
                    description: "Free ESPN API for scores, stats, news".into(),
                },
                DataSourceStatus {
                    name: "Sleeper".into(),
                    available: false,
                    requires_key: false,
                    key_configured: true,
                    description: "Free Sleeper API for injuries, player stats".into(),
                },
                DataSourceStatus {
                    name: "API-Sports".into(),
                    available: false,
                    requires_key: true,
                    key_configured: !config.api_sports_key.is_empty(),
                    description: "Optional paid API for structured NFL data (100 req/day free)".into(),
                },
            ];
        }
    };

    vec![
        DataSourceStatus {
            name: "ESPN".into(),
            available: client.check_espn_available().await,
            requires_key: false,
            key_configured: true,
            description: "Free ESPN API for scores, stats, news".into(),
        },
        DataSourceStatus {
            name: "Sleeper".into(),
            available: client.check_sleeper_available().await,
            requires_key: false,
            key_configured: true,
            description: "Free Sleeper API for injuries, player stats".into(),
        },
        DataSourceStatus {
            name: "API-Sports".into(),
            available: client.check_api_sports_available().await,
            requires_key: true,
            key_configured: !config.api_sports_key.is_empty(),
            description: "Optional paid API for structured NFL data (100 req/day free)".into(),
        },
    ]
}

/// Filter the Sleeper players response to only injured players.
/// Sleeper returns a massive JSON object keyed by player ID.
/// We extract players with a non-null injury status.
fn filter_injured_players(players: &Value) -> Vec<Value> {
    let mut injured = Vec::new();

    if let Some(obj) = players.as_object() {
        for (_id, player) in obj {
            let injury_status = player
                .get("injury_status")
                .and_then(|v| v.as_str());
            let has_injury = injury_status.is_some()
                && injury_status != Some("Active")
                && injury_status != Some("Questionable");

            if has_injury {
                let name = player
                    .get("full_name")
                    .or_else(|| player.get("first_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");
                let team = player
                    .get("team")
                    .and_then(|v| v.as_str())
                    .unwrap_or("UNK");
                let position = player
                    .get("position")
                    .and_then(|v| v.as_str())
                    .unwrap_or("UNK");
                let injury_body_part = player
                    .get("injury_body_part")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");
                let injury_notes = player
                    .get("injury_notes")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                injured.push(serde_json::json!({
                    "name": name,
                    "team": team,
                    "position": position,
                    "injury_status": injury_status.unwrap_or("Unknown"),
                    "injury_body_part": injury_body_part,
                    "injury_notes": injury_notes,
                }));
            }
        }
    }

    // Sort by team then name for consistent output
    injured.sort_by(|a, b| {
        let a_team = a.get("team").and_then(|v| v.as_str()).unwrap_or("");
        let b_team = b.get("team").and_then(|v| v.as_str()).unwrap_or("");
        let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        (a_team, a_name).cmp(&(b_team, b_name))
    });

    injured
}
