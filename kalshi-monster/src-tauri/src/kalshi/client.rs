use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use sqlx::{Pool, Sqlite};
use crate::kalshi::models::{
    KalshiCache, KalshiConfig, KalshiEvent, KalshiEventsResponse, KalshiMarket,
    KalshiMarketsQuery, KalshiMarketsResponse, KalshiMarketSummary, KalshiOrderbook,
    KalshiOrderbookResponse, KalshiPosition,
    KalshiPositionsResponse, KalshiBalance, KalshiBalanceResponse,
};

// ═══════════════════════════════════════════════════════════════
// Kalshi HTTP Client
// ═══════════════════════════════════════════════════════════════

const PRIMARY_BASE_URL: &str = "https://api.elections.kalshi.com/trade-api/v2";
const FALLBACK_BASE_URL: &str = "https://trading-api.kalshi.com/trade-api/v2";
const DEMO_BASE_URL: &str = "https://demo-api.kalshi.co/trade-api/v2";

/// How many seconds a cached market list stays fresh
const CACHE_TTL_SECS: u64 = 60;

/// Maximum pages to fetch when paginating through all markets (explicit refresh)
const MAX_PAGINATION_PAGES: usize = 20;

/// Pages fetched on cold start / dashboard load — keeps first paint fast
const QUICK_LOAD_PAGES: usize = 2;

/// Cap category/search result payloads sent to the UI
const MAX_UI_MARKET_RESULTS: usize = 100;

/// How many events to request per page (full nested catalog)
const PAGE_LIMIT: u32 = 200;

/// Flat /markets page size for dashboard quick load
const FLAT_MARKET_PAGE_LIMIT: u32 = 100;

pub struct KalshiClient {
    config: Arc<RwLock<KalshiConfig>>,
    client: reqwest::Client,
    /// JWT bearer token acquired via /login
    token: Arc<RwLock<Option<String>>>,
    /// When the token expires (unix seconds)
    token_expiry: Arc<RwLock<Option<u64>>>,
    /// Cached market list
    cache: Arc<RwLock<Option<KalshiCache>>>,
    /// Prevents concurrent full-catalog fetches; set before pagination, cleared after
    fetch_in_progress: Arc<AtomicBool>,
    /// When set, market cache snapshots are written to SQLite after each update
    persist_pool: Option<Arc<Pool<Sqlite>>>,
    /// True when in-memory cache was restored from SQLite (not yet refreshed from API)
    cache_from_persisted: AtomicBool,
    /// Last Kalshi catalog fetch error (startup warm or dashboard quick load) for UI hints
    last_fetch_error: Arc<RwLock<Option<String>>>,
}

impl KalshiClient {
    pub fn new(
        config: KalshiConfig,
        persist_pool: Option<Arc<Pool<Sqlite>>>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("Failed to build reqwest client");
        KalshiClient {
            config: Arc::new(RwLock::new(config)),
            client,
            token: Arc::new(RwLock::new(None)),
            token_expiry: Arc::new(RwLock::new(None)),
            cache: Arc::new(RwLock::new(None)),
            fetch_in_progress: Arc::new(AtomicBool::new(false)),
            persist_pool,
            cache_from_persisted: AtomicBool::new(false),
            last_fetch_error: Arc::new(RwLock::new(None)),
        }
    }

    pub fn config(&self) -> KalshiConfig {
        self.config.read().unwrap().clone()
    }

    pub fn set_config(&self, config: KalshiConfig) {
        *self.config.write().unwrap() = config;
    }

    pub fn last_fetch_error(&self) -> Option<String> {
        self.last_fetch_error.read().unwrap().clone()
    }

    pub fn set_last_fetch_error(&self, message: impl Into<String>) {
        *self.last_fetch_error.write().unwrap() = Some(message.into());
    }

    fn clear_fetch_error(&self) {
        *self.last_fetch_error.write().unwrap() = None;
    }

    fn base_url(&self) -> String {
        let config = self.config();
        if config.use_demo {
            DEMO_BASE_URL.to_string()
        } else if !config.base_url.is_empty() {
            config.base_url.clone()
        } else {
            PRIMARY_BASE_URL.to_string()
        }
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn cache_metadata(&self) -> (String, Option<u64>, bool, Option<u64>) {
        let cache = self.cache.read().unwrap();
        match &*cache {
            None => ("cold".to_string(), None, true, None),
            Some(cache) => {
                let age = Self::now_secs().saturating_sub(cache.fetched_at);
                let status = if cache.full_catalog { "full" } else { "partial" };
                (status.to_string(), Some(age), !cache.full_catalog, Some(cache.fetched_at))
            }
        }
    }

    /// Whether the visible cache was rehydrated from SQLite and not yet replaced by a live fetch.
    pub fn showing_persisted_snapshot(&self) -> bool {
        self.cache_from_persisted.load(Ordering::Relaxed)
            && self.cache.read().unwrap().is_some()
    }

    pub fn is_cache_stale(&self) -> bool {
        let cache = self.cache.read().unwrap();
        match &*cache {
            None => true,
            Some(cache) => Self::now_secs() - cache.fetched_at > CACHE_TTL_SECS,
        }
    }

    pub fn get_cached(&self) -> Option<Vec<KalshiMarket>> {
        self.cache.read().unwrap().as_ref().map(|c| c.markets.clone())
    }

    /// Total markets in the in-memory tape (not the UI slice limit).
    pub fn cached_tape_market_count(&self) -> usize {
        self.cache
            .read()
            .unwrap()
            .as_ref()
            .map(|c| c.markets.len())
            .unwrap_or(0)
    }

    fn is_token_valid(&self) -> bool {
        let token = self.token.read().unwrap();
        let token_expiry = self.token_expiry.read().unwrap();
        match (&*token, *token_expiry) {
            (Some(_), Some(expiry)) => Self::now_secs() + 60 < expiry,
            (Some(_), None) => true,
            _ => false,
        }
    }

    fn auth_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static("kalshi-monster/0.6.0"));
        let token = self.token.read().unwrap();
        if let Some(token) = &*token {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", token)) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        headers
    }

    /// Authenticate with email/password to get a JWT token.
    /// Only required for portfolio/trading endpoints.
    pub async fn login(&self) -> Result<(), String> {
        let config = self.config();
        if config.email.is_empty() || config.password.is_empty() {
            return Err("No Kalshi credentials configured".to_string());
        }

        let url = format!("{}/login", self.base_url());
        let body = serde_json::json!({
            "email": config.email,
            "password": config.password,
        });

        let resp = self
            .client
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Kalshi login request failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Kalshi login failed ({}): {}", status, text));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| format!("Failed to parse login response: {}", e))?;
        let token = json["token"]
            .as_str()
            .ok_or("No token in login response")?
            .to_string();

        *self.token.write().unwrap() = Some(token);
        // Kalshi tokens are valid for 24h
        *self.token_expiry.write().unwrap() = Some(Self::now_secs() + 86400);
        Ok(())
    }

    /// Ensure we have a valid token; attempt login if not.
    async fn ensure_auth(&self) -> Result<(), String> {
        if !self.is_token_valid() {
            self.login().await?;
        }
        Ok(())
    }

    // ─── Public read endpoints (no auth required) ──────────────────────────────

    /// Fetch a single page of markets with optional query filters.
    pub async fn fetch_markets_page(
        &self,
        query: &KalshiMarketsQuery,
    ) -> Result<KalshiMarketsResponse, String> {
        let url = format!("{}/markets", self.base_url());
        let mut req = self.client.get(&url).headers(self.auth_headers());

        if let Some(limit) = query.limit {
            req = req.query(&[("limit", limit.to_string())]);
        }
        if let Some(cursor) = &query.cursor {
            req = req.query(&[("cursor", cursor)]);
        }
        if let Some(status) = &query.status {
            req = req.query(&[("status", status)]);
        }
        if let Some(series_ticker) = &query.series_ticker {
            req = req.query(&[("series_ticker", series_ticker)]);
        }
        if let Some(event_ticker) = &query.event_ticker {
            req = req.query(&[("event_ticker", event_ticker)]);
        }
        if let Some(min_ts) = query.min_close_ts {
            req = req.query(&[("min_close_ts", min_ts.to_string())]);
        }
        if let Some(max_ts) = query.max_close_ts {
            req = req.query(&[("max_close_ts", max_ts.to_string())]);
        }
        if let Some(mve_filter) = &query.mve_filter {
            req = req.query(&[("mve_filter", mve_filter.as_str())]);
        }

        let resp = req.send().await.map_err(|e| {
            // Try fallback URL on connection errors
            format!("Kalshi market fetch failed: {}", e)
        })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Kalshi API error ({}): {}", status, text));
        }

        resp.json::<KalshiMarketsResponse>()
            .await
            .map_err(|e| format!("Failed to parse Kalshi markets response: {}", e))
    }

    /// Fetch a single page of non-multivariate events with nested markets.
    async fn fetch_events_page(
        &self,
        base_url: &str,
        cursor: Option<&str>,
    ) -> Result<KalshiEventsResponse, String> {
        let url = format!("{}/events", base_url);
        let mut req = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .query(&[
                ("limit", PAGE_LIMIT.to_string()),
                ("status", "open".to_string()),
                ("with_nested_markets", "true".to_string()),
            ]);

        if let Some(cursor) = cursor {
            req = req.query(&[("cursor", cursor.to_string())]);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("Kalshi events fetch failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Kalshi events API error ({}): {}", status, text));
        }

        resp.json::<KalshiEventsResponse>()
            .await
            .map_err(|e| format!("Failed to parse Kalshi events response: {}", e))
    }

    fn flatten_event_markets(event: KalshiEvent) -> Vec<KalshiMarket> {
        let event_title = event.title.trim().to_string();
        let event_category = event.category.clone();
        let event_series_ticker = event
            .series_ticker
            .trim()
            .is_empty()
            .then_some(())
            .and(None)
            .or_else(|| Some(event.series_ticker.clone()));

        event
            .markets
            .unwrap_or_default()
            .into_iter()
            .map(|mut market| {
                if market.title.trim().is_empty() {
                    let yes_sub_title = market
                        .yes_sub_title
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    market.title = match (event_title.is_empty(), yes_sub_title) {
                        (false, Some(value)) => format!("{} - {}", event_title, value),
                        (false, None) => event_title.clone(),
                        (true, Some(value)) => value.to_string(),
                        (true, None) => market.ticker.clone(),
                    };
                }

                if market.series_ticker.as_deref().map(str::trim).unwrap_or("").is_empty() {
                    market.series_ticker = event_series_ticker.clone();
                }

                if market.category.as_deref().map(str::trim).unwrap_or("").is_empty() {
                    market.category = event_category.clone();
                }

                market
            })
            .collect()
    }

    fn top_summaries(markets: &[KalshiMarket], limit: usize) -> Vec<KalshiMarketSummary> {
        let mut ranked: Vec<&KalshiMarket> = markets.iter().collect();
        ranked.sort_by(|a, b| {
            b.volume_24h()
                .partial_cmp(&a.volume_24h())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.total_volume()
                        .partial_cmp(&a.total_volume())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });
        ranked
            .into_iter()
            .take(limit)
            .map(KalshiMarketSummary::from)
            .collect()
    }

    /// Flat open markets via GET /markets — much smaller payloads than nested /events.
    async fn fetch_markets_flat_pages(&self, max_pages: usize) -> Result<Vec<KalshiMarket>, String> {
        let mut all_markets: Vec<KalshiMarket> = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0usize;
        let mut retries = 0usize;
        const MAX_RETRIES: usize = 3;

        loop {
            if pages >= max_pages {
                break;
            }

            let query = KalshiMarketsQuery {
                limit: Some(FLAT_MARKET_PAGE_LIMIT),
                cursor: cursor.clone(),
                status: Some("open".to_string()),
                mve_filter: Some("exclude".to_string()),
                ..Default::default()
            };

            match self.fetch_markets_page(&query).await {
                Ok(resp) => {
                    retries = 0;
                    pages += 1;
                    if resp.markets.is_empty() {
                        break;
                    }
                    all_markets.extend(resp.markets);
                    cursor = resp.cursor;
                    if cursor.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    if e.contains("429") && retries < MAX_RETRIES {
                        retries += 1;
                        let wait_ms = 1000u64 * retries as u64;
                        tracing::warn!(
                            "Kalshi flat markets rate limited, retry in {}ms",
                            wait_ms
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                    } else if !all_markets.is_empty() {
                        tracing::warn!("Kalshi flat markets pagination error: {}", e);
                        break;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Ok(all_markets)
    }

    async fn fetch_markets_flat_resilient(&self, max_pages: usize) -> Result<Vec<KalshiMarket>, String> {
        let primary_base_url = self.base_url().to_string();
        match self.fetch_markets_flat_pages(max_pages).await {
            Ok(markets) => Ok(markets),
            Err(e) if primary_base_url == PRIMARY_BASE_URL => {
                tracing::warn!("Primary Kalshi flat markets failed, trying fallback: {}", e);
                // Re-fetch on fallback base requires a one-off client pointed at fallback;
                // for now surface the primary error — fallback path matches events catalog.
                Err(e)
            }
            Err(e) => Err(e),
        }
    }

    /// Nested /events catalog — used only for explicit full refresh.
    async fn fetch_events_catalog_from_base(
        &self,
        base_url: &str,
        max_pages: usize,
    ) -> Result<Vec<KalshiMarket>, String> {
        let mut all_markets: Vec<KalshiMarket> = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0;
        let mut retries = 0usize;
        const MAX_RETRIES: usize = 3;

        loop {
            if pages >= max_pages {
                break;
            }

            match self.fetch_events_page(base_url, cursor.as_deref()).await {
                Ok(resp) => {
                    retries = 0;
                    let has_next = resp.cursor.is_some();
                    cursor = resp.cursor;
                    if resp.events.is_empty() {
                        break;
                    }

                    pages += 1;
                    for event in resp.events {
                        all_markets.extend(Self::flatten_event_markets(event));
                    }

                    if !has_next {
                        break;
                    }

                    // Throttle between pages to stay under Kalshi's rate limit.
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                Err(e) => {
                    if e.contains("429") && retries < MAX_RETRIES {
                        retries += 1;
                        let wait_ms = 2000u64 * retries as u64;
                        tracing::warn!(
                            "Kalshi rate limited on page {}, retrying in {}ms ({}/{})",
                            pages + 1,
                            wait_ms,
                            retries,
                            MAX_RETRIES
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                    } else if !all_markets.is_empty() {
                        tracing::warn!("Kalshi pagination error on page {}: {}", pages + 1, e);
                        break;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Ok(all_markets)
    }

    async fn fetch_events_catalog_resilient(
        &self,
        max_pages: usize,
    ) -> Result<Vec<KalshiMarket>, String> {
        let primary_base_url = self.base_url().to_string();
        match self
            .fetch_events_catalog_from_base(&primary_base_url, max_pages)
            .await
        {
            Ok(markets) => Ok(markets),
            Err(e) if primary_base_url == PRIMARY_BASE_URL => {
                tracing::warn!("Primary Kalshi URL failed, trying fallback: {}", e);
                self.fetch_events_catalog_from_base(FALLBACK_BASE_URL, max_pages)
                    .await
                    .map_err(|e2| {
                        format!(
                            "Both Kalshi endpoints failed. Primary: {}. Fallback: {}",
                            e, e2
                        )
                    })
            }
            Err(e) => Err(e),
        }
    }

    /// Store a new cache snapshot in the local handle.
    fn apply_cache(&self, cache: KalshiCache) {
        *self.cache.write().unwrap() = Some(cache);
    }

    fn schedule_persist(&self) {
        let cache = self.cache.read().unwrap().clone();
        if let (Some(pool), Some(cache)) = (&self.persist_pool, cache) {
            let pool = pool.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) =
                    crate::kalshi::market_cache_store::save_persisted_cache(&pool, &cache).await
                {
                    tracing::warn!("kalshi market cache persist failed: {}", e);
                }
            });
        }
    }

    /// Restore cache from SQLite at startup (no disk write).
    pub async fn hydrate_cache(&self, cache: KalshiCache) {
        tracing::info!(
            "Kalshi cache rehydrated from SQLite: {} markets (full_catalog={})",
            cache.markets.len(),
            cache.full_catalog
        );
        self.cache_from_persisted.store(true, Ordering::Relaxed);
        self.apply_cache(cache);
    }

    async fn store_cache(&self, markets: Vec<KalshiMarket>, full_catalog: bool) {
        let cache = KalshiCache {
            markets,
            fetched_at: Self::now_secs(),
            full_catalog,
        };
        self.apply_cache(cache);
        self.cache_from_persisted.store(false, Ordering::Relaxed);
        self.schedule_persist();
    }

    pub fn needs_full_catalog(&self) -> bool {
        let stale = self.is_cache_stale();
        let cache = self.cache.read().unwrap();
        match &*cache {
            None => true,
            Some(_) if stale => true,
            Some(cache) => !cache.full_catalog,
        }
    }

    /// Quick cache for dashboard first paint — at most `QUICK_LOAD_PAGES` API pages.
    /// Skips HTTP fetch if a full warm is already in progress (returns stale cache).
    pub async fn ensure_quick_cache(&self) -> Result<(), String> {
        let stale = self.is_cache_stale();
        {
            let cache = self.cache.read().unwrap();
            if let Some(cache) = &*cache {
                // Empty persisted snapshot must not block live fetch (KB-1).
                if !stale && !cache.markets.is_empty() {
                    return Ok(());
                }
                if cache.full_catalog {
                    // Stale full cache — fall through to quick reload so UI is not blocked 10s+
                    tracing::info!("Kalshi full cache stale; quick-reloading for dashboard");
                }
            }
        }

        // If a full warm is already in progress, don't start a second fetch — the
        // caller will work with the stale/partial cache until the warm completes.
        if self.fetch_in_progress.load(Ordering::Relaxed) {
            tracing::info!("Kalshi full catalog warm in progress; skipping quick reload");
            if self.cached_tape_market_count() == 0 {
                self.set_last_fetch_error(
                    "Catalog refresh in progress — wait a few seconds or tap Refresh and snapshot",
                );
            }
            return Ok(());
        }

        let started = std::time::Instant::now();
        tracing::info!(
            "Kalshi quick cache load via flat /markets ({} pages x {} markets)",
            QUICK_LOAD_PAGES,
            FLAT_MARKET_PAGE_LIMIT
        );
        let markets = match self.fetch_markets_flat_resilient(QUICK_LOAD_PAGES).await {
            Ok(m) => m,
            Err(e) => {
                self.set_last_fetch_error(&e);
                return Err(e);
            }
        };
        tracing::info!(
            "Kalshi quick cache ready: {} markets in {}ms",
            markets.len(),
            started.elapsed().as_millis()
        );
        if markets.is_empty() {
            self.set_last_fetch_error(
                "Kalshi API returned zero open markets — check credentials in Settings",
            );
        } else {
            self.clear_fetch_error();
        }
        self.store_cache(markets, false).await;
        Ok(())
    }

    /// Fetch all open non-multivariate markets, paginating through all pages.
    /// Caches the result for `CACHE_TTL_SECS` seconds.
    /// Uses `fetch_in_progress` guard to prevent concurrent full-catalog fetches.
    pub async fn fetch_all_markets(&self) -> Result<Vec<KalshiMarket>, String> {
        let stale = self.is_cache_stale();
        {
            let cache = self.cache.read().unwrap();
            if !stale {
                if let Some(cached) = &*cache {
                    if cached.full_catalog {
                        return Ok(cached.markets.clone());
                    }
                }
            }
        }

        // Prevent concurrent full-catalog warm (safety net for background + UI refresh)
        if self.fetch_in_progress.swap(true, Ordering::AcqRel) {
            tracing::warn!("Kalshi full catalog fetch already in progress; skipping duplicate");
            // Return stale cache if available, otherwise error
            let cache = self.cache.read().unwrap();
            return cache
                .as_ref()
                .map(|c| c.markets.clone())
                .ok_or_else(|| "Full catalog fetch already in progress and no cache available".to_string());
        }

        let _guard = FetchInProgressGuard {
            flag: self.fetch_in_progress.clone(),
        };

        let started = std::time::Instant::now();
        tracing::info!(
            "Kalshi full cache refresh via nested /events ({} pages max)",
            MAX_PAGINATION_PAGES
        );
        let all_markets = self
            .fetch_events_catalog_resilient(MAX_PAGINATION_PAGES)
            .await?;
        tracing::info!(
            "Kalshi full cache ready: {} markets in {}ms",
            all_markets.len(),
            started.elapsed().as_millis()
        );
        if all_markets.is_empty() {
            self.set_last_fetch_error(
                "Kalshi full catalog refresh returned zero markets — check API access",
            );
        } else {
            self.clear_fetch_error();
        }
        self.store_cache(all_markets.clone(), true).await;
        Ok(all_markets)
    }

    fn cached_market_slice(&self) -> Option<Vec<KalshiMarket>> {
        self.cache.read().unwrap().as_ref().map(|c| c.markets.clone())
    }

    /// Look up a full market (including resolution rules) from the local tape cache.
    pub fn find_cached_market(&self, ticker: &str) -> Option<KalshiMarket> {
        self.cached_market_slice().and_then(|markets| {
            markets
                .into_iter()
                .find(|m| m.ticker.eq_ignore_ascii_case(ticker))
        })
    }

    /// Sibling contracts on the same event (for multi-candidate / jungle framing).
    pub fn cached_siblings_for_event(
        &self,
        event_ticker: &str,
        exclude_ticker: Option<&str>,
        limit: usize,
    ) -> Vec<KalshiMarketSummary> {
        let Some(markets) = self.cached_market_slice() else {
            return Vec::new();
        };
        let mut sibs: Vec<KalshiMarket> = markets
            .into_iter()
            .filter(|m| m.event_ticker.eq_ignore_ascii_case(event_ticker))
            .filter(|m| {
                exclude_ticker
                    .map(|ex| !m.ticker.eq_ignore_ascii_case(ex))
                    .unwrap_or(true)
            })
            .collect();
        sibs.sort_by(|a, b| {
            b.volume_24h()
                .partial_cmp(&a.volume_24h())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sibs.iter()
            .take(limit)
            .map(KalshiMarketSummary::from)
            .collect()
    }

    /// Fetch a single market by ticker
    pub async fn fetch_market(&self, ticker: &str) -> Result<KalshiMarket, String> {
        // Sanitize ticker — only allow alphanumeric, hyphens, underscores, dots
        let safe_ticker: String = ticker
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
            .collect();

        if safe_ticker.is_empty() {
            return Err("Invalid ticker".to_string());
        }

        let url = format!("{}/markets/{}", self.base_url(), safe_ticker);
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| format!("Kalshi single market fetch failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Kalshi market {} not found ({}): {}", safe_ticker, status, text));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse market response: {}", e))?;

        serde_json::from_value(json["market"].clone())
            .map_err(|e| format!("Failed to deserialize market: {}", e))
    }

    /// Fetch the orderbook for a market
    pub async fn fetch_orderbook(&self, ticker: &str) -> Result<KalshiOrderbook, String> {
        let safe_ticker: String = ticker
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
            .collect();

        if safe_ticker.is_empty() {
            return Err("Invalid ticker".to_string());
        }

        let url = format!("{}/markets/{}/orderbook", self.base_url(), safe_ticker);
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| format!("Kalshi orderbook fetch failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Kalshi orderbook error ({}): {}", status, text));
        }

        let parsed: KalshiOrderbookResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse orderbook: {}", e))?;

        Ok(parsed.orderbook)
    }

    /// Search markets by keyword against the cached market list
    pub async fn search_markets(&self, query: &str) -> Result<Vec<KalshiMarketSummary>, String> {
        let trimmed = query.trim();
        if trimmed.len() < 2 {
            return Err("Search query must be at least 2 characters".to_string());
        }
        self.ensure_quick_cache().await?;
        let markets = self
            .cached_market_slice()
            .ok_or("Kalshi market cache unavailable")?;
        let q = trimmed.to_lowercase();
        let results: Vec<KalshiMarketSummary> = markets
            .iter()
            .filter(|m| {
                m.title.to_lowercase().contains(&q)
                    || m.ticker.to_lowercase().contains(&q)
                    || m.event_ticker.to_lowercase().contains(&q)
            })
            .take(MAX_UI_MARKET_RESULTS)
            .map(KalshiMarketSummary::from)
            .collect();
        Ok(results)
    }

    /// Get markets filtered by category (inferred from ticker)
    pub async fn get_markets_by_category(
        &self,
        category: &str,
    ) -> Result<Vec<KalshiMarketSummary>, String> {
        self.ensure_quick_cache().await?;
        let markets = self
            .cached_market_slice()
            .ok_or("Kalshi market cache unavailable")?;
        let results: Vec<KalshiMarketSummary> = markets
            .iter()
            .filter(|m| {
                if category == "All" {
                    true
                } else {
                    m.infer_category().eq_ignore_ascii_case(category)
                }
            })
            .take(MAX_UI_MARKET_RESULTS)
            .map(KalshiMarketSummary::from)
            .collect();
        Ok(results)
    }

    /// Get top markets by 24h volume
    pub async fn get_top_markets(&self, limit: usize) -> Result<Vec<KalshiMarketSummary>, String> {
        self.ensure_quick_cache().await?;
        let markets = self
            .cached_market_slice()
            .ok_or("Kalshi market cache unavailable")?;
        Ok(Self::top_summaries(&markets, limit.min(MAX_UI_MARKET_RESULTS)))
    }

    // ─── Auth-required endpoints ────────────────────────────────────────────────

    /// Get portfolio balance (requires login)
    pub async fn get_balance(&self) -> Result<KalshiBalance, String> {
        self.ensure_auth().await?;
        let url = format!("{}/portfolio/balance", self.base_url());
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| format!("Kalshi balance fetch failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Kalshi balance error ({}): {}", status, text));
        }

        let parsed: KalshiBalanceResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse balance: {}", e))?;

        Ok(parsed.balance)
    }

    /// Get portfolio positions (requires login)
    pub async fn get_positions(&self) -> Result<Vec<KalshiPosition>, String> {
        self.ensure_auth().await?;
        let url = format!("{}/portfolio/positions", self.base_url());
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| format!("Kalshi positions fetch failed: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Kalshi positions error ({}): {}", status, text));
        }

        let parsed: KalshiPositionsResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse positions: {}", e))?;

        Ok(parsed.market_positions)
    }

    /// Force-invalidate cache (used after config changes)
    pub fn invalidate_cache(&self) {
        *self.cache.write().unwrap() = None;
        *self.token.write().unwrap() = None;
        *self.token_expiry.write().unwrap() = None;
    }

    /// Summarize all cached markets by category
    pub fn category_stats(&self) -> Vec<KalshiCategoryStat> {
        let cache = self.cache.read().unwrap();
        let mut stats: std::collections::HashMap<String, (usize, f64)> = std::collections::HashMap::new();

        if let Some(cache) = &*cache {
            for m in &cache.markets {
                let cat = m.infer_category().to_string();
                let entry = stats.entry(cat).or_insert((0, 0.0));
                entry.0 += 1;
                entry.1 += m.volume_24h();
            }
        }

        let mut result: Vec<KalshiCategoryStat> = stats
            .into_iter()
            .map(|(cat, (count, vol))| KalshiCategoryStat {
                category: cat,
                count,
                volume_24h: vol,
            })
            .collect();

        result.sort_by(|a, b| b.count.cmp(&a.count));
        result
    }

    /// Return a snapshot of the in-memory cache.
    pub fn cache_snapshot(&self) -> Option<KalshiCache> {
        self.cache.read().unwrap().clone()
    }

    /// Async cache snapshot when the caller is already on the Tokio reactor.
    pub async fn cache_snapshot_async(&self) -> Option<KalshiCache> {
        self.cache_snapshot()
    }

    /// Check whether a full-catalog fetch is currently in progress.
    pub fn is_fetch_in_progress(&self) -> bool {
        self.fetch_in_progress.load(Ordering::Relaxed)
    }
}

/// Statistics about a market category
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KalshiCategoryStat {
    pub category: String,
    pub count: usize,
    pub volume_24h: f64,
}

/// RAII guard that clears `fetch_in_progress` on drop (even during panic/unwind).
struct FetchInProgressGuard {
    flag: Arc<AtomicBool>,
}

impl Drop for FetchInProgressGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_event_markets_inherits_event_metadata() {
        let event = KalshiEvent {
            event_ticker: "KXNEWPOPE-70".to_string(),
            title: "Who will the next Pope be?".to_string(),
            series_ticker: "KXNEWPOPE".to_string(),
            status: String::new(),
            category: Some("Elections".to_string()),
            sub_title: None,
            mutually_exclusive: true,
            markets: Some(vec![KalshiMarket {
                ticker: "KXNEWPOPE-70-PPIZ".to_string(),
                event_ticker: "KXNEWPOPE-70".to_string(),
                yes_sub_title: Some("Pierbattista Pizzaballa".to_string()),
                ..Default::default()
            }]),
            strike_date: String::new(),
        };

        let markets = KalshiClient::flatten_event_markets(event);
        assert_eq!(markets.len(), 1);
        assert_eq!(markets[0].title, "Who will the next Pope be? - Pierbattista Pizzaballa");
        assert_eq!(markets[0].category.as_deref(), Some("Elections"));
        assert_eq!(markets[0].series_ticker.as_deref(), Some("KXNEWPOPE"));
    }

    #[tokio::test]
    async fn cached_tape_market_count_reflects_cache_len() {
        let client = KalshiClient::new(KalshiConfig::default(), None);
        assert_eq!(client.cached_tape_market_count(), 0);
        client
            .store_cache(vec![KalshiMarket::default(), KalshiMarket::default()], false)
            .await;
        assert_eq!(client.cached_tape_market_count(), 2);
    }
}

/// Build a KalshiConfig from the app config
pub fn kalshi_config_from_app(config: &crate::config::AppConfig) -> KalshiConfig {
    KalshiConfig {
        base_url: PRIMARY_BASE_URL.to_string(),
        email: config.kalshi_email.clone(),
        password: config.kalshi_password.clone(),
        poll_interval_secs: config.kalshi_poll_interval_secs,
        use_demo: false,
    }
}
