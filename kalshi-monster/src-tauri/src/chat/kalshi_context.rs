use crate::kalshi::client::KalshiClient;
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════
// Kalshi Market Context Builder
// Builds a data-rich context for AI chat focused on prediction markets.
// Sports data is only included when the user explicitly asks about sports.
// ═══════════════════════════════════════════════════════════════

/// Structured context for a single market analysis decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiMarketDecision {
    pub ticker: String,
    pub market_title: String,
    pub category: String,
    pub contract_side: String, // YES, NO, or PASS
    pub market_price_pct: f64,
    pub fair_probability_pct: f64,
    pub edge_points: f64,
    pub spread_cents: f64,
    pub liquidity_score: f64,
    pub ev_per_contract_cents: f64,
    pub ev_roi_pct: f64,
    pub raw_kelly_pct: f64,
    pub fractional_kelly_pct: f64,
    pub recommended_stake_dollars: f64,
    pub max_position_dollars: f64,
    pub decision: String, // TAKE, WATCH, PASS
    pub confidence_tier: String,
    pub thesis: String,
    pub evidence: Vec<String>,
    pub risk_flags: Vec<String>,
    pub data_quality: String,
    pub price_to_enter: f64,
}

/// Complete snapshot of the current Kalshi environment for the AI
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KalshiContextSnapshot {
    pub top_volume_markets: Vec<MarketBrief>,
    pub trending_markets: Vec<MarketBrief>,
    pub category_stats: Vec<CategorySnapshot>,
    pub selected_market: Option<MarketDetail>,
    pub portfolio: Option<PortfolioSnapshot>,
    pub orderbook: Option<OrderbookSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketBrief {
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub yes_prob_pct: f64,
    pub spread_cents: f64,
    pub volume_24h: f64,
    pub liquidity: f64,
    pub close_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDetail {
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub yes_ask: f64,
    pub yes_bid: f64,
    pub no_ask: f64,
    pub no_bid: f64,
    pub yes_prob_pct: f64,
    pub spread: f64,
    pub liquidity: f64,
    pub volume_24h: f64,
    pub close_time: Option<String>,
    pub rules_primary: String,
    pub is_provisional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorySnapshot {
    pub name: String,
    pub market_count: usize,
    pub total_volume_24h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioSnapshot {
    pub balance_dollars: f64,
    pub positions: Vec<PositionSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionSnapshot {
    pub ticker: String,
    pub side: String,
    pub exposure_dollars: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookSnapshot {
    pub yes_bids: Vec<(f64, f64)>,
    pub yes_asks: Vec<(f64, f64)>,
    pub no_bids: Vec<(f64, f64)>,
    pub no_asks: Vec<(f64, f64)>,
    pub best_yes_bid: f64,
    pub best_yes_ask: f64,
    pub best_no_bid: f64,
    pub best_no_ask: f64,
    pub total_liquidity: f64,
}

/// Backend signal for Analyst UI when live Kalshi tape is missing or stale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiChatContextStatus {
    pub degraded: bool,
    pub tape_market_count: usize,
    pub reasons: Vec<String>,
}

/// Assess whether chat can inject meaningful Kalshi market context (KB-2a).
pub fn assess_kalshi_chat_context(client: &KalshiClient) -> KalshiChatContextStatus {
    let tape_market_count = client.cached_tape_market_count();
    let mut reasons = Vec::new();
    if tape_market_count == 0 {
        reasons.push(
            "Kalshi market tape is empty — open Markets and refresh before relying on analysis."
                .to_string(),
        );
        if let Some(err) = client.last_fetch_error() {
            reasons.push(format!("Last catalog fetch error: {err}"));
        }
    }
    let degraded = !reasons.is_empty();
    KalshiChatContextStatus {
        degraded,
        tape_market_count,
        reasons,
    }
}

/// Build a rich Kalshi context string for the AI prompt.
/// This is the primary context builder for the chat pipeline.
pub async fn build_kalshi_context(
    client: &mut KalshiClient,
    user_message: &str,
    portfolio: Option<&PortfolioSnapshot>,
) -> String {
    let mut ctx = String::with_capacity(8192);

    ctx.push_str("# KALSHI MARKET INTELLIGENCE CONTEXT\n\n");

    // Determine whether the user referenced a specific Kalshi ticker.
    let _is_specific_ticker = extract_ticker_from_query(user_message).is_some();

    // Top volume markets (always included)
    match client.get_top_markets(20).await {
        Ok(markets) => {
            ctx.push_str("## TOP VOLUME MARKETS (Last 24h)\n");
            for m in markets.iter().take(10) {
                ctx.push_str(&format!("- [{}] {} — Cat: {}, Yes: {:.1}%, Spread: {:.0}c, Volume: ${:.0}\n",
                    m.ticker, m.title, m.category, m.yes_prob_pct, m.spread * 100.0, m.volume_24h));
            }
            ctx.push('\n');
        }
        Err(_) => {
            ctx.push_str("## TOP VOLUME MARKETS\n(unavailable)\n\n");
        }
    }

    // Category stats (always included)
    match fetch_category_stats(client).await {
        Ok(stats) => {
            ctx.push_str("## CATEGORY OVERVIEW\n");
            for s in stats {
                ctx.push_str(&format!("- {}: {} markets, ${:.0} 24h volume\n", s.name, s.market_count, s.total_volume_24h));
            }
            ctx.push('\n');
        }
        Err(_) => {
            ctx.push_str("## CATEGORY OVERVIEW\n(unavailable)\n\n");
        }
    }

    // If user mentions a specific ticker or market, fetch detailed data
    if let Some(ticker) = extract_ticker_from_query(user_message) {
        match client.fetch_market(&ticker).await {
            Ok(market) => {
                ctx.push_str("## SELECTED MARKET DETAIL\n");
                ctx.push_str(&format!("- Ticker: {}\n", market.ticker));
                ctx.push_str(&format!("- Title: {}\n", market.display_title()));
                ctx.push_str(&format!("- Category: {}\n", market.infer_category()));
                ctx.push_str(&format!("- Status: {}\n", market.status));
                ctx.push_str(&format!("- YES Ask: ${:.2}, YES Bid: ${:.2}\n", market.yes_ask(), market.yes_bid()));
                ctx.push_str(&format!("- NO Ask: ${:.2}, NO Bid: ${:.2}\n",
                    market.no_ask_dollars.parse::<f64>().unwrap_or(0.0),
                    market.no_bid_dollars.parse::<f64>().unwrap_or(0.0)));
                ctx.push_str(&format!("- Spread: {:.0}c\n", market.yes_spread() * 100.0));
                ctx.push_str(&format!("- Implied YES prob: {:.1}%\n", market.yes_prob_pct()));
                ctx.push_str(&format!("- Liquidity: ${:.0}\n", market.liquidity()));
                ctx.push_str(&format!("- 24h Volume: ${:.0}\n", market.volume_24h()));
                if let Some(close) = &market.close_time {
                    ctx.push_str(&format!("- Closes: {}\n", close));
                }
                if !market.rules_primary.is_empty() {
                    let rules = if market.rules_primary.len() > 500 {
                        format!("{}...", &market.rules_primary[..500])
                    } else {
                        market.rules_primary.clone()
                    };
                    ctx.push_str(&format!("- Rules: {}\n", rules));
                }
                ctx.push('\n');
            }
            Err(e) => {
                ctx.push_str(&format!("## SELECTED MARKET DETAIL\nError fetching {}: {}\n\n", ticker, e));
            }
        }
    }

    // Portfolio context (only if credentials exist and portfolio is provided)
    if let Some(portfolio) = portfolio {
        ctx.push_str("## PORTFOLIO CONTEXT\n");
        ctx.push_str(&format!("- Available balance: ${:.2}\n", portfolio.balance_dollars));
        if !portfolio.positions.is_empty() {
            ctx.push_str("- Open positions:\n");
            for pos in &portfolio.positions {
                ctx.push_str(&format!("  - {} {}: ${:.2} exposure\n", pos.ticker, pos.side, pos.exposure_dollars));
            }
        } else {
            ctx.push_str("- No open positions\n");
        }
        ctx.push('\n');
    }

    // Sports-specific context is intentionally kept out of the Kalshi market snapshot.
    // The chat pipeline injects sports data only when the user explicitly asks for it
    // (see openrouter::build_sports_context). This keeps non-sports prediction-market
    // prompts focused on the retrieved Kalshi tape.

    ctx.push_str("## TRADING SAFETY REMINDERS\n");
    ctx.push_str("- This is an analysis tool. No orders are placed automatically.\n");
    ctx.push_str("- Always verify prices on kalshi.com before trading.\n");
    ctx.push_str("- Never stake more than you can afford to lose.\n");
    ctx.push_str("- Outcomes are probabilistic, never guaranteed.\n\n");

    ctx
}

/// Determine if the user is asking about sports markets.
/// Kept for test coverage; live chat sports detection lives in the openrouter pipeline.
#[cfg(test)]
fn is_sports_market_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    // Tightened to explicit leagues, sports, positions, and stat/mechanics terms.
    // Generic words like "team", "game", "score", "championship", or "series" are
    // intentionally excluded so non-sports prediction markets (politics, economics,
    // weather, finance) do not trigger irrelevant sports data injection.
    let sports_keywords = [
        "sports", "nba", "nfl", "mlb", "nhl", "ufc", "golf", "tennis",
        "basketball", "baseball", "football", "hockey",
        "quarterback", "qb", "running back", "rb", "wide receiver", "wr",
        "passing", "rushing", "receiving", "yards", "touchdown",
        "tip-off", "kickoff", "overtime", "halftime",
    ];
    sports_keywords.iter().any(|kw| lower.contains(kw))
}

/// Extract a Kalshi ticker from a user query (e.g., "KX-" or "KXEVENT-" prefix)
fn extract_ticker_from_query(query: &str) -> Option<String> {
    // Try to find tickers like KX-XXXX or KXEVENT-XXXX
    let words: Vec<&str> = query.split_whitespace().collect();
    for word in words {
        let w = word.trim().trim_matches(|c| c == '.' || c == ',' || c == '!' || c == '?');
        if w.starts_with("KX") || w.starts_with("kx") {
            return Some(w.to_uppercase());
        }
    }
    None
}

/// Fetch top markets by category
async fn fetch_category_stats(client: &mut KalshiClient) -> Result<Vec<CategorySnapshot>, String> {
    let stats = client.category_stats();
    Ok(stats.into_iter().map(|s| CategorySnapshot {
        name: s.category,
        market_count: s.count as usize,
        total_volume_24h: s.volume_24h,
    }).collect())
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_sports_market_query() {
        assert!(is_sports_market_query("What do you think about NFL markets?"));
        assert!(is_sports_market_query("Analyze the NBA playoff props"));
        assert!(is_sports_market_query("Passing yards prop for Mahomes"));
        assert!(!is_sports_market_query("What is the Fed rate outlook?"));
        assert!(!is_sports_market_query("Analyze crypto markets"));
        assert!(!is_sports_market_query("Political election predictions"));
        assert!(!is_sports_market_query("Which team will win the championship series?"));
    }

    #[test]
    fn test_extract_ticker_from_query() {
        assert_eq!(extract_ticker_from_query("Analyze KX-FED-25DEC"), Some("KX-FED-25DEC".to_string()));
        assert_eq!(extract_ticker_from_query("What about kx-nba-2025?"), Some("KX-NBA-2025".to_string()));
        assert_eq!(extract_ticker_from_query("No ticker here"), None);
    }

    #[test]
    fn test_non_sports_query_no_sports_injected() {
        // This test proves that sports data is not injected for non-sports queries
        let query = "Analyze the Federal Reserve decision market";
        assert!(!is_sports_market_query(query));
    }

    #[test]
    fn assess_chat_context_degraded_when_tape_empty() {
        use std::sync::Arc;
        use tokio::sync::RwLock;
        use crate::kalshi::KalshiConfig;
        let shared = Arc::new(RwLock::new(None));
        let client = KalshiClient::new(KalshiConfig::default(), shared, None);
        let status = assess_kalshi_chat_context(&client);
        assert!(status.degraded);
        assert_eq!(status.tape_market_count, 0);
        assert!(!status.reasons.is_empty());
    }
}
