use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════
// Kalshi Trading API v2 — Data Models
// Base URL: https://trading-api.kalshi.com/trade-api/v2
// ═══════════════════════════════════════════════════════════════

/// Kalshi client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiConfig {
    /// Primary API base URL
    pub base_url: String,
    /// Account email for JWT auth (optional — public endpoints don't require it)
    pub email: String,
    /// Account password for JWT auth
    pub password: String,
    /// How often to refresh the market cache (seconds)
    pub poll_interval_secs: u64,
    /// Use the Kalshi demo environment
    pub use_demo: bool,
}

impl Default for KalshiConfig {
    fn default() -> Self {
        KalshiConfig {
            base_url: "https://trading-api.kalshi.com/trade-api/v2".to_string(),
            email: String::new(),
            password: String::new(),
            poll_interval_secs: 60,
            use_demo: false,
        }
    }
}

/// A single Kalshi binary market
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KalshiMarket {
    /// Unique market ticker, e.g. "KXNBAPTS-26MAY25NYKCLE-CLEDMITCHELL45-20"
    pub ticker: String,
    /// Parent event ticker, e.g. "KXNBAPTS-26MAY25NYKCLE"
    pub event_ticker: String,
    /// Human-readable title
    pub title: String,
    /// Market status: "active" | "closed" | "settled"
    #[serde(default)]
    pub status: String,
    /// Time the market closes to new orders
    pub close_time: Option<String>,
    /// Settlement expiration time
    pub expiration_time: Option<String>,
    /// "binary" or "scalar"
    #[serde(default = "default_binary")]
    pub market_type: String,
    /// YES ask price in dollars (e.g., "0.5500")
    #[serde(default)]
    pub yes_ask_dollars: String,
    /// YES bid price in dollars
    #[serde(default)]
    pub yes_bid_dollars: String,
    /// NO ask price in dollars
    #[serde(default)]
    pub no_ask_dollars: String,
    /// NO bid price in dollars
    #[serde(default)]
    pub no_bid_dollars: String,
    /// Last traded price in dollars
    #[serde(default)]
    pub last_price_dollars: String,
    /// 24h trading volume (fractional units)
    #[serde(default)]
    pub volume_24h_fp: String,
    /// Total volume (fractional units)
    #[serde(default)]
    pub volume_fp: String,
    /// Total liquidity in dollars
    #[serde(default)]
    pub liquidity_dollars: String,
    /// Open interest (fractional units)
    #[serde(default)]
    pub open_interest_fp: String,
    /// Whether fractional trading is enabled
    #[serde(default)]
    pub fractional_trading_enabled: bool,
    /// YES side subtitle
    pub yes_sub_title: Option<String>,
    /// NO side subtitle
    pub no_sub_title: Option<String>,
    /// Market rules
    #[serde(default)]
    pub rules_primary: String,
    /// Settlement result ("Yes" | "No" | "")
    #[serde(default)]
    pub result: String,
    /// Whether the market can close early
    #[serde(default)]
    pub can_close_early: bool,
    /// Series/category ticker
    pub series_ticker: Option<String>,
    /// Event category propagated from the events endpoint when available
    pub category: Option<String>,
    /// Market notional value in dollars
    pub notional_value_dollars: Option<String>,
    /// YES ask size
    pub yes_ask_size_fp: Option<String>,
    /// YES bid size
    pub yes_bid_size_fp: Option<String>,
    /// Whether this is an MVE (Multi-Variate Event) market
    #[serde(default)]
    pub is_provisional: bool,
}

impl KalshiMarket {
    /// Parse yes_ask_dollars as f64
    pub fn yes_ask(&self) -> f64 {
        self.yes_ask_dollars.parse().unwrap_or(0.0)
    }

    /// Parse yes_bid_dollars as f64
    pub fn yes_bid(&self) -> f64 {
        self.yes_bid_dollars.parse().unwrap_or(0.0)
    }

    /// Midpoint price between yes ask and yes bid
    pub fn yes_mid(&self) -> f64 {
        let ask = self.yes_ask();
        let bid = self.yes_bid();
        if ask > 0.0 && bid > 0.0 {
            (ask + bid) / 2.0
        } else if ask > 0.0 {
            ask
        } else {
            bid
        }
    }

    /// Implied probability percentage (0–100) for YES
    pub fn yes_prob_pct(&self) -> f64 {
        let mid = self.yes_mid();
        if mid <= 0.0 {
            // Try last price
            let last: f64 = self.last_price_dollars.parse().unwrap_or(0.0);
            return last * 100.0;
        }
        mid * 100.0
    }

    /// Best-effort display title for market lists.
    pub fn display_title(&self) -> String {
        let title = self.title.trim();
        if !title.is_empty() {
            return title.to_string();
        }

        if let Some(yes_sub_title) = self
            .yes_sub_title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return yes_sub_title.to_string();
        }

        self.ticker.clone()
    }

    /// Volume as f64
    pub fn volume_24h(&self) -> f64 {
        self.volume_24h_fp.parse().unwrap_or(0.0)
    }

    /// Total volume as f64
    pub fn total_volume(&self) -> f64 {
        self.volume_fp.parse().unwrap_or(0.0)
    }

    /// Liquidity as f64
    pub fn liquidity(&self) -> f64 {
        self.liquidity_dollars.parse().unwrap_or(0.0)
    }

    /// Bid-ask spread on YES side
    pub fn yes_spread(&self) -> f64 {
        let ask = self.yes_ask();
        let bid = self.yes_bid();
        if ask > 0.0 && bid > 0.0 {
            ask - bid
        } else {
            0.0
        }
    }

    /// Infer a display category from Kalshi metadata, then fall back to ticker prefixes.
    pub fn infer_category(&self) -> &'static str {
        if let Some(category) = self.category.as_deref() {
            match category.trim().to_ascii_lowercase().as_str() {
                "sports" | "esports" => return "Sports",
                "elections" | "politics" => return "Politics",
                "economics" => return "Economics",
                "crypto" => return "Crypto",
                "financials" | "companies" | "finance" => return "Finance",
                "climate and weather" | "weather" => return "Weather",
                _ => {}
            }
        }

        let t = format!(
            "{} {}",
            self.event_ticker,
            self.series_ticker.as_deref().unwrap_or("")
        )
        .to_uppercase();

        if t.contains("NBA") || t.contains("NFL") || t.contains("MLB") || t.contains("NHL")
            || t.contains("ATP") || t.contains("WTA") || t.contains("UFC") || t.contains("WNBA")
            || t.contains("UCL") || t.contains("SPORTS") || t.contains("GOLF") || t.contains("PGA")
            || t.contains("MATCH") || t.contains("SETWINNER") || t.contains("EXACTMATCH")
        {
            "Sports"
        } else if t.contains("PRES") || t.contains("SENATE") || t.contains("HOUSE")
            || t.contains("ELECTION") || t.contains("GOV") || t.contains("VOTE")
            || t.contains("POTUS") || t.contains("CONGRESS")
        {
            "Politics"
        } else if t.contains("FED") || t.contains("CPI") || t.contains("GDP")
            || t.contains("RATE") || t.contains("INFLATION") || t.contains("ECON")
            || t.contains("UNEMPLOYMENT") || t.contains("JOBS")
        {
            "Economics"
        } else if t.contains("BTC") || t.contains("ETH") || t.contains("DOGE")
            || t.contains("SOL") || t.contains("XRP") || t.contains("CRYPTO")
        {
            "Crypto"
        } else if t.contains("SPX") || t.contains("NASDAQ") || t.contains("STOCK")
            || t.contains("SPY") || t.contains("QQQ") || t.contains("TSLA")
            || t.contains("AAPL") || t.contains("MARKET")
        {
            "Finance"
        } else if t.contains("WEATHER") || t.contains("TEMP") || t.contains("SNOW") {
            "Weather"
        } else {
            "Other"
        }
    }
}

fn default_binary() -> String {
    "binary".to_string()
}

/// Response from GET /markets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiMarketsResponse {
    pub cursor: Option<String>,
    #[serde(default)]
    pub markets: Vec<KalshiMarket>,
}

/// A single Kalshi event (a group of related markets)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KalshiEvent {
    pub event_ticker: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub series_ticker: String,
    #[serde(default)]
    pub status: String,
    pub category: Option<String>,
    pub sub_title: Option<String>,
    #[serde(default)]
    pub mutually_exclusive: bool,
    pub markets: Option<Vec<KalshiMarket>>,
    #[serde(default)]
    pub strike_date: String,
}

/// Response from GET /events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiEventsResponse {
    pub cursor: Option<String>,
    #[serde(default)]
    pub events: Vec<KalshiEvent>,
}

/// Single orderbook level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiOrderbookLevel {
    /// Price in cents (0–100)
    pub price: i64,
    /// Quantity at this level
    pub delta: i64,
}

/// Full orderbook for a market
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiOrderbook {
    pub yes: Vec<KalshiOrderbookLevel>,
    pub no: Vec<KalshiOrderbookLevel>,
}

/// Wrapped orderbook response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiOrderbookResponse {
    pub orderbook: KalshiOrderbook,
}

/// Portfolio balance (requires auth)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiBalance {
    /// Available balance in cents
    pub balance: i64,
    /// Reserved for open orders in cents
    pub reserved_fees: Option<i64>,
}

/// Portfolio balance response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiBalanceResponse {
    pub balance: KalshiBalance,
}

/// An open position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiPosition {
    pub ticker: String,
    pub position: i64,       // positive = YES, negative = NO
    pub market_exposure: i64, // in cents
    pub resting_orders_count: i64,
    pub realized_pnl: Option<i64>,
    pub total_traded: Option<i64>,
}

/// Positions response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiPositionsResponse {
    pub cursor: Option<String>,
    #[serde(default)]
    pub market_positions: Vec<KalshiPosition>,
}

/// Condensed market data returned to the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiMarketSummary {
    pub ticker: String,
    pub event_ticker: String,
    pub title: String,
    pub category: String,
    pub status: String,
    pub yes_prob_pct: f64,
    pub yes_ask: f64,
    pub yes_bid: f64,
    pub no_ask: f64,
    pub no_bid: f64,
    pub last_price: f64,
    pub volume_24h: f64,
    pub total_volume: f64,
    pub liquidity: f64,
    pub spread: f64,
    pub close_time: Option<String>,
    pub expiration_time: Option<String>,
    pub result: String,
    pub can_close_early: bool,
    pub is_provisional: bool,
}

impl From<&KalshiMarket> for KalshiMarketSummary {
    fn from(m: &KalshiMarket) -> Self {
        KalshiMarketSummary {
            ticker: m.ticker.clone(),
            event_ticker: m.event_ticker.clone(),
            title: m.display_title(),
            category: m.infer_category().to_string(),
            status: m.status.clone(),
            yes_prob_pct: m.yes_prob_pct(),
            yes_ask: m.yes_ask(),
            yes_bid: m.yes_bid(),
            no_ask: m.no_ask_dollars.parse().unwrap_or(0.0),
            no_bid: m.no_bid_dollars.parse().unwrap_or(0.0),
            last_price: m.last_price_dollars.parse().unwrap_or(0.0),
            volume_24h: m.volume_24h(),
            total_volume: m.total_volume(),
            liquidity: m.liquidity(),
            spread: m.yes_spread(),
            close_time: m.close_time.clone(),
            expiration_time: m.expiration_time.clone(),
            result: m.result.clone(),
            can_close_early: m.can_close_early,
            is_provisional: m.is_provisional,
        }
    }
}

/// Query params for fetching markets
#[derive(Debug, Clone, Default)]
pub struct KalshiMarketsQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub status: Option<String>,
    pub series_ticker: Option<String>,
    pub event_ticker: Option<String>,
    pub min_close_ts: Option<i64>,
    pub max_close_ts: Option<i64>,
    /// `only` | `exclude` — dashboard quick load uses `exclude` for non-combo markets
    pub mve_filter: Option<String>,
}

/// Cached markets data with timestamp
#[derive(Debug, Clone)]
pub struct KalshiCache {
    pub markets: Vec<KalshiMarket>,
    pub fetched_at: u64,
    /// `false` when populated via quick dashboard load (partial catalog)
    pub full_catalog: bool,
}

// ── Kalshi Prediction Tracking ──

/// A prediction made on a Kalshi market
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiPrediction {
    pub id: String,
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub predicted_probability: f64,
    pub actual_outcome: Option<String>,
    pub confidence_score: Option<u8>,
    pub reasoning: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
    pub stake_amount: f64,
    pub pnl: Option<f64>,
    pub pick_type: Option<String>,
    pub price_to_enter: Option<f64>,
    pub market_price_at_entry: Option<f64>,
    pub contract_side: Option<String>,
    pub edge_points: Option<f64>,
    pub fractional_kelly_pct: Option<f64>,
    pub recommended_stake_dollars: Option<f64>,
    pub risk_flags: Option<Vec<String>>,
    pub thesis: Option<String>,
    pub data_quality: Option<String>,
    pub decision: Option<String>,
}

/// Stats for Kalshi predictions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiPredictionStats {
    pub total: u32,
    pub wins: u32,
    pub losses: u32,
    pub pending: u32,
    pub win_rate: f64,
    pub avg_confidence_score: f64,
    pub total_volume_traded: f64,
    pub total_pnl: f64,
    pub roi_pct: f64,
    pub calibration: CalibrationMetrics,
    pub category_breakdown: std::collections::HashMap<String, CategoryStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryStats {
    pub count: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationMetrics {
    pub brier_score: f64,
    pub brier_skill_score: f64,
    pub calibration_slope: f64,
    pub calibration_intercept: f64,
}

/// Result of grading a single prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiGradingResult {
    pub prediction_id: String,
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub predicted_probability: f64,
    pub actual_outcome: String,
    pub outcome: String,
    pub pnl: f64,
    pub stake_amount: f64,
    pub contract_side: Option<String>,
    pub market_price_at_entry: Option<f64>,
    pub notes: Option<String>,
    pub resolved_at: String,
}

/// Summary of a grading run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiGradingSummary {
    pub total_predictions: u32,
    pub pending_gradable: u32,
    pub graded: u32,
    pub wins: u32,
    pub losses: u32,
    pub total_pnl: f64,
    pub results: Vec<KalshiGradingResult>,
    pub fetched_at: String,
}
