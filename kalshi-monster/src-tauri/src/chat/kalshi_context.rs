use crate::kalshi::client::KalshiClient;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::market_gate::{
    assess_market_gate_for_ticker, format_gate_line, MarketGate,
};

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

/// Max characters of resolution rules injected per market (full rules preferred over vague summary).
const RULES_INJECT_MAX_CHARS: usize = 2500;

/// Format one retrieved-market row for the analyst prompt.
/// Always prints bid / ask / mid dollars separately from mid-implied percent.
pub fn format_retrieved_market_line(
    m: &crate::kalshi::KalshiMarketSummary,
    status: &str,
    result: &str,
    gate: &MarketGate,
) -> String {
    let mid = if m.yes_bid > 0.0 && m.yes_ask > 0.0 {
        (m.yes_bid + m.yes_ask) / 2.0
    } else if m.yes_ask > 0.0 {
        m.yes_ask
    } else if m.yes_bid > 0.0 {
        m.yes_bid
    } else {
        m.yes_prob_pct / 100.0
    };
    let spread_c = m.spread * 100.0;
    let wide = if spread_c > 10.0 {
        " ⚠ WIDE_SPREAD"
    } else {
        ""
    };
    let thin = if m.volume_24h <= 0.0 && m.liquidity <= 0.0 {
        " ⚠ THIN/NO_VOLUME"
    } else {
        ""
    };
    format!(
        "- [{}] {} — Event: {}, Cat: {}, Status: {}, Result: {}, \
         Yes bid ${:.4} / ask ${:.4} / mid ${:.4} (mid-implied {:.2}%), \
         Spread: {:.1}c, Vol24h: ${:.0}, Liq: ${:.0}, GATE={}{}{}\n",
        m.ticker,
        m.title,
        m.event_ticker,
        m.category,
        status,
        result,
        m.yes_bid,
        m.yes_ask,
        mid,
        m.yes_prob_pct,
        spread_c,
        m.volume_24h,
        m.liquidity,
        gate.label(),
        wide,
        thin,
    )
}

/// Format resolution rules for prompt injection (truncate long legal text safely).
pub fn format_resolution_rules(rules: &str, max_chars: usize) -> String {
    let trimmed = rules.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let char_len = trimmed.chars().count();
    if char_len <= max_chars {
        return trimmed.to_string();
    }
    // Prefer cutting on a word boundary near the limit (char-safe for UTF-8).
    let mut truncated: String = trimmed.chars().take(max_chars).collect();
    if let Some(idx) = truncated.rfind(|c: char| c.is_whitespace()) {
        if idx > max_chars / 2 {
            truncated.truncate(idx);
        }
    }
    format!("{}… [rules truncated]", truncated.trim_end())
}

fn append_resolution_rules_block(ctx: &mut String, ticker: &str, title: &str, rules: &str, provisional: bool, can_close_early: bool) {
    ctx.push_str(&format!("### {} — {}\n", ticker, title));
    if provisional {
        ctx.push_str("- Flag: PROVISIONAL settlement\n");
    }
    if can_close_early {
        ctx.push_str("- Flag: can close early\n");
    }
    let formatted = format_resolution_rules(rules, RULES_INJECT_MAX_CHARS);
    if formatted.is_empty() {
        ctx.push_str("- Resolution rules: (missing from tape — reduce confidence; do not invent criteria)\n");
    } else {
        ctx.push_str("- Resolution rules (authoritative):\n");
        ctx.push_str(&formatted);
        ctx.push('\n');
    }
    ctx.push('\n');
}

/// Result of building Kalshi chat context (tape + gates + open-market set for web search).
#[derive(Debug, Clone, Default)]
pub struct KalshiContextBuild {
    pub context: String,
    /// (ticker, title) for markets that passed OPEN gate — eligible for web search.
    pub open_markets: Vec<(String, String)>,
    pub gates: Vec<(String, MarketGate)>,
}

/// Build a rich Kalshi context string for the AI prompt.
/// Query-aware: prefer ticker detail + keyword-matched markets over dumping the whole tape.
pub async fn build_kalshi_context(
    client: &KalshiClient,
    user_message: &str,
    portfolio: Option<&PortfolioSnapshot>,
) -> String {
    build_kalshi_context_full(client, user_message, portfolio)
        .await
        .context
}

/// Full build including settlement gates and open-market list for gated web search.
pub async fn build_kalshi_context_full(
    client: &KalshiClient,
    user_message: &str,
    portfolio: Option<&PortfolioSnapshot>,
) -> KalshiContextBuild {
    let mut ctx = String::with_capacity(12288);
    let mut open_markets: Vec<(String, String)> = Vec::new();
    let mut gates: Vec<(String, MarketGate)> = Vec::new();
    let now = Utc::now();

    ctx.push_str("# KALSHI MARKET INTELLIGENCE CONTEXT\n\n");
    ctx.push_str(
        "Use ONLY markets listed below (plus any SELECTED MARKET DETAIL / RESOLUTION RULES). \
         Do not invent tickers, prices, or settlement criteria. Prefer fewer markets with complete analysis.\n\n",
    );
    ctx.push_str(
        "## RESOLUTION DISCIPLINE (read first)\n\
         - Quote the injected resolution rules before sizing any TAKE.\n\
         - Multi-candidate / \"jungle\" markets (e.g. primaries): YES pays only if the **named candidate** \
           (or exact contract definition) wins — not the party, not \"someone like them\". Sibling contracts \
           are mutually exclusive unless rules say otherwise.\n\
         - Ambiguous or missing rules → PASS or WATCH with AmbiguousResolution; never invent criteria.\n\
         - Prices in this context are in **dollars** ($0.00–$1.00) unless labeled as percent.\n\
         - SETTLEMENT GATES are authoritative: GATE=SETTLED or GATE=CLOSED → FORCE PASS. \
           Never invent \"open field\" fair value for a finished event.\n\n",
    );

    let keywords = retrieval_keywords(user_message);
    let specific = extract_ticker_from_query(user_message);
    let mut rule_tickers: Vec<String> = Vec::new();

    // Top / retrieved markets (bounded) — mid-prob re-rank + open-first, not pure volume longshots
    let allow_longshots = wants_longshot_scan(user_message);
    let mut selected_for_siblings: Vec<crate::kalshi::KalshiMarketSummary> = Vec::new();
    match client.get_top_markets(80).await {
        Ok(markets) => {
            let selected = select_markets_for_query(
                &markets,
                &keywords,
                specific.as_deref(),
                8,
                allow_longshots,
                now,
            );
            selected_for_siblings = selected.clone();
            ctx.push_str("## RETRIEVED MARKETS (query-filtered, open/mid-prob preferred)\n");
            if allow_longshots {
                ctx.push_str("(longshot scan enabled by query)\n");
            }
            if selected.is_empty() {
                ctx.push_str("(no matching markets — report tape miss; do not invent contracts)\n");
            } else {
                for m in &selected {
                    // Prefer full cache row for result/status/close
                    let full = client.find_cached_market(&m.ticker);
                    let status = full
                        .as_ref()
                        .map(|f| f.status.as_str())
                        .unwrap_or(m.status.as_str());
                    let result = full
                        .as_ref()
                        .map(|f| f.result.as_str())
                        .unwrap_or(m.result.as_str());
                    let close = full
                        .as_ref()
                        .and_then(|f| f.close_time.as_deref())
                        .or(m.close_time.as_deref());
                    let exp = full
                        .as_ref()
                        .and_then(|f| f.expiration_time.as_deref())
                        .or(m.expiration_time.as_deref());
                    let gate = assess_market_gate_for_ticker(
                        Some(&m.ticker),
                        status,
                        result,
                        close,
                        exp,
                        now,
                    );
                    gates.push((m.ticker.clone(), gate.clone()));
                    if gate.allows_take() {
                        open_markets.push((m.ticker.clone(), m.title.clone()));
                    }
                    // Never pair YES *ask* dollars with *mid* percent — wide books
                    // (e.g. bid $0.01 / ask $0.94 / mid 48%) used to print as
                    // "Yes: $0.9400 (48.00%)" and derail the model into data-bug debates.
                    ctx.push_str(&format_retrieved_market_line(
                        m,
                        status,
                        if result.is_empty() { "—" } else { result },
                        &gate,
                    ));
                    // Collect top matches for rules injection (selected ticker first)
                    if rule_tickers.len() < 3 {
                        rule_tickers.push(m.ticker.clone());
                    }
                }
            }
            ctx.push('\n');
        }
        Err(_) => {
            ctx.push_str("## RETRIEVED MARKETS\n(unavailable — refresh Command desk)\n\n");
        }
    }

    // Sibling field for multi-candidate events (jungle / nominee books)
    inject_sibling_field_context(client, &selected_for_siblings, &mut ctx, now, &mut gates);

    // Compact category overview (top 6 by count)
    match fetch_category_stats(client).await {
        Ok(mut stats) => {
            stats.sort_by(|a, b| {
                b.total_volume_24h
                    .partial_cmp(&a.total_volume_24h)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            ctx.push_str("## CATEGORY OVERVIEW (top by volume)\n");
            for s in stats.into_iter().take(6) {
                ctx.push_str(&format!(
                    "- {}: {} markets, ${:.0} 24h volume\n",
                    s.name, s.market_count, s.total_volume_24h
                ));
            }
            ctx.push('\n');
        }
        Err(_) => {
            ctx.push_str("## CATEGORY OVERVIEW\n(unavailable)\n\n");
        }
    }

    // Prefer explicit ticker for selected detail; fall back to best retrieved match
    let detail_ticker = specific.clone().or_else(|| rule_tickers.first().cloned());

    if let Some(ticker) = detail_ticker {
        // Prefer cache (includes rules when catalog was nested); fall back to live fetch
        let market = if let Some(m) = client.find_cached_market(&ticker) {
            Ok(m)
        } else {
            client.fetch_market(&ticker).await
        };

        match market {
            Ok(market) => {
                let gate = assess_market_gate_for_ticker(
                    Some(&market.ticker),
                    &market.status,
                    &market.result,
                    market.close_time.as_deref(),
                    market.expiration_time.as_deref(),
                    now,
                );
                if !gates.iter().any(|(t, _)| t.eq_ignore_ascii_case(&market.ticker)) {
                    gates.push((market.ticker.clone(), gate.clone()));
                }
                if gate.allows_take()
                    && !open_markets
                        .iter()
                        .any(|(t, _)| t.eq_ignore_ascii_case(&market.ticker))
                {
                    open_markets.push((market.ticker.clone(), market.display_title()));
                }

                ctx.push_str("## SELECTED MARKET DETAIL\n");
                ctx.push_str(&format!("- Ticker: {}\n", market.ticker));
                ctx.push_str(&format!("- Event: {}\n", market.event_ticker));
                ctx.push_str(&format!("- Title: {}\n", market.display_title()));
                if let Some(yes_sub) = market.yes_sub_title.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    ctx.push_str(&format!("- YES subtitle (candidate/outcome label): {}\n", yes_sub));
                }
                ctx.push_str(&format!("- Category: {}\n", market.infer_category()));
                ctx.push_str(&format!("- Status: {}\n", market.status));
                ctx.push_str(&format!(
                    "- Settlement result: {}\n",
                    if market.result.is_empty() {
                        "—"
                    } else {
                        market.result.as_str()
                    }
                ));
                ctx.push_str(&format!("- GATE: {} — {}\n", gate.label(), match &gate {
                    MarketGate::Open => "eligible for TAKE".to_string(),
                    MarketGate::Settled { reason, .. } | MarketGate::Closed { reason } => {
                        format!("FORCE PASS ({reason})")
                    }
                }));
                ctx.push_str(&format!(
                    "- YES Ask: ${:.4}, YES Bid: ${:.4}\n",
                    market.yes_ask(),
                    market.yes_bid()
                ));
                ctx.push_str(&format!(
                    "- NO Ask: ${:.4}, NO Bid: ${:.4}\n",
                    market.no_ask_dollars.parse::<f64>().unwrap_or(0.0),
                    market.no_bid_dollars.parse::<f64>().unwrap_or(0.0)
                ));
                ctx.push_str(&format!("- Spread: {:.1}c\n", market.yes_spread() * 100.0));
                ctx.push_str(&format!(
                    "- Implied YES: ${:.4} ({:.2}%)\n",
                    market.yes_mid(),
                    market.yes_prob_pct()
                ));
                ctx.push_str(&format!("- Liquidity: ${:.0}\n", market.liquidity()));
                ctx.push_str(&format!("- 24h Volume: ${:.0}\n", market.volume_24h()));
                if let Some(close) = &market.close_time {
                    ctx.push_str(&format!("- Closes: {}\n", close));
                }
                ctx.push('\n');

                ctx.push_str("## RESOLUTION RULES (selected market)\n");
                append_resolution_rules_block(
                    &mut ctx,
                    &market.ticker,
                    &market.display_title(),
                    &market.rules_primary,
                    market.is_provisional,
                    market.can_close_early,
                );
            }
            Err(e) => {
                ctx.push_str(&format!(
                    "## SELECTED MARKET DETAIL\nError fetching {}: {}\n\n",
                    ticker, e
                ));
            }
        }
    }

    // Inject rules for additional top retrieved markets (sibling candidates / comparison set)
    let extra: Vec<String> = rule_tickers
        .into_iter()
        .filter(|t| specific.as_ref().map(|s| !s.eq_ignore_ascii_case(t)).unwrap_or(true))
        .take(2)
        .collect();
    if !extra.is_empty() {
        ctx.push_str("## RESOLUTION RULES (related retrieved markets)\n");
        ctx.push_str(
            "Use these to enforce mutual exclusivity / jungle framing against the selected market.\n\n",
        );
        for t in extra {
            if let Some(m) = client.find_cached_market(&t) {
                append_resolution_rules_block(
                    &mut ctx,
                    &m.ticker,
                    &m.display_title(),
                    &m.rules_primary,
                    m.is_provisional,
                    m.can_close_early,
                );
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

    // Settlement gate summary (authoritative)
    if !gates.is_empty() {
        ctx.push_str("## SETTLEMENT GATES (authoritative — override narrative)\n");
        for (ticker, gate) in &gates {
            ctx.push_str(&format_gate_line(ticker, gate));
            ctx.push('\n');
        }
        ctx.push_str(
            "If GATE=SETTLED or CLOSED: decision must be PASS (or WATCH only for re-open risk). \
             Do not assign open-field fair values to finished primaries/elections.\n\n",
        );
    }

    ctx.push_str("## TRADING SAFETY REMINDERS\n");
    ctx.push_str("- This is an analysis tool. No orders are placed automatically.\n");
    ctx.push_str("- Always verify prices on kalshi.com before trading.\n");
    ctx.push_str("- Never stake more than you can afford to lose.\n");
    ctx.push_str("- Outcomes are probabilistic, never guaranteed.\n");
    ctx.push_str("- App post-process caps fractional Kelly and stake (default ≤5% bankroll); do not recommend full-Kelly long-shots.\n");
    ctx.push_str("- price_to_enter must be dollars on [0,1]; market_price_pct is percent 0–100 of $1.\n");
    ctx.push_str(
        "- PRICE LABELS: bid/ask/mid are dollars; mid-implied % is from the mid only. \
         A wide book (e.g. bid $0.01 / ask $0.94 / mid ~$0.48) is NOT a data bug — it is unexecutable friction. Prefer PASS/WATCH.\n",
    );
    ctx.push_str(
        "- EXECUTABILITY: Do not TAKE when spread > edge, volume is zero, or the book is flagged WIDE_SPREAD. \
         Prefer tight-spread, non-zero volume markets for any TAKE.\n",
    );
    ctx.push_str("- WEB EVIDENCE (if present) is optional grounding only — never overrides GATE or rules.\n\n");

    KalshiContextBuild {
        context: ctx,
        open_markets,
        gates,
    }
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

fn retrieval_keywords(query: &str) -> Vec<String> {
    let stop: std::collections::HashSet<&str> = [
        "the", "a", "an", "and", "or", "of", "to", "for", "on", "in", "is", "are", "what",
        "which", "most", "today", "with", "vs", "versus", "me", "my", "your", "this", "that",
        "from", "into", "about", "show", "give", "find", "analyze", "analysis", "market",
        "markets", "kalshi", "price", "prices", "any", "have", "has", "do", "does", "how",
    ]
    .into_iter()
    .collect();
    query
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= 3 && !stop.contains(w.as_str()))
        .take(12)
        .collect()
}

/// User explicitly wants extreme longshots / penny contracts.
pub fn wants_longshot_scan(query: &str) -> bool {
    let q = query.to_lowercase();
    [
        "longshot",
        "long shot",
        "lottery",
        "penny",
        "extreme",
        "tail",
        "dark horse",
        "sub 1%",
        "sub-1",
        "under 1%",
        "99%",
        "0.1%",
    ]
    .iter()
    .any(|k| q.contains(k))
}

fn select_markets_for_query(
    markets: &[crate::kalshi::KalshiMarketSummary],
    keywords: &[String],
    ticker: Option<&str>,
    limit: usize,
    allow_longshots: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<crate::kalshi::KalshiMarketSummary> {
    let mut scored: Vec<(i32, &crate::kalshi::KalshiMarketSummary)> = markets
        .iter()
        .filter_map(|m| {
            let gate = assess_market_gate_for_ticker(
                Some(&m.ticker),
                &m.status,
                &m.result,
                m.close_time.as_deref(),
                m.expiration_time.as_deref(),
                now,
            );
            // Prefer open markets; only keep settled if explicit ticker match
            let explicit = ticker.map(|t| m.ticker.eq_ignore_ascii_case(t)).unwrap_or(false);
            if !gate.allows_take() && !explicit {
                return None;
            }

            let hay = format!(
                "{} {} {} {}",
                m.ticker.to_lowercase(),
                m.title.to_lowercase(),
                m.category.to_lowercase(),
                m.event_ticker.to_lowercase()
            );
            let mut score = 0i32;
            if explicit {
                score += 1000;
            }
            for kw in keywords {
                if hay.contains(kw) {
                    score += 10;
                }
            }

            // Mid-probability band preferred for "mispriced" scans (real edge space).
            let p = m.yes_prob_pct;
            if (10.0..=90.0).contains(&p) {
                score += 25;
            } else if (5.0..=95.0).contains(&p) {
                score += 12;
            } else if !allow_longshots {
                // Extreme tails: heavy penalty unless user asked for longshots
                score -= 40;
            } else {
                score += 5; // mild boost when longshot mode
            }

            // Tight spread bonus / wide spread penalty (spread is in dollars 0–1).
            // Unexecutable books (50c+ spreads) must not surface as "mispriced" candidates.
            let spread_c = m.spread * 100.0;
            if spread_c <= 2.0 {
                score += 12;
            } else if spread_c <= 5.0 {
                score += 5;
            } else if spread_c <= 10.0 {
                score -= 5;
            } else if spread_c <= 25.0 {
                score -= 25;
            } else {
                score -= 50;
            }

            // Liquidity / volume priors — zero-volume rows are noise for fair-value work
            if m.volume_24h <= 0.0 {
                score -= 20;
            } else {
                score += (m.volume_24h.log10().max(0.0) as i32).min(10);
            }
            if m.liquidity > 0.0 {
                score += (m.liquidity.log10().max(0.0) as i32).min(6);
            } else {
                score -= 8;
            }

            if !gate.allows_take() {
                score -= 100; // explicit ticker settled still ranks low
            }

            Some((score, m))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let any_kw = scored.iter().any(|(s, _)| *s >= 10);
    scored
        .into_iter()
        .filter(|(s, _)| {
            if any_kw {
                *s >= 10 || ticker.is_some()
            } else {
                // Default scan: keep only non-negative scores (filters pure extreme junk)
                *s >= 0 || allow_longshots
            }
        })
        .take(limit)
        .map(|(_, m)| m.clone())
        .collect()
}

/// Inject sibling contracts for multi-candidate events so the model sees the full field.
fn inject_sibling_field_context(
    client: &KalshiClient,
    selected: &[crate::kalshi::KalshiMarketSummary],
    ctx: &mut String,
    now: chrono::DateTime<chrono::Utc>,
    gates: &mut Vec<(String, MarketGate)>,
) {
    use std::collections::HashSet;
    let mut seen_events = HashSet::new();
    let mut block = String::new();
    for m in selected.iter().take(4) {
        if m.event_ticker.is_empty() || !seen_events.insert(m.event_ticker.to_uppercase()) {
            continue;
        }
        let sibs = client.cached_siblings_for_event(&m.event_ticker, Some(&m.ticker), 8);
        if sibs.is_empty() {
            continue;
        }
        block.push_str(&format!(
            "### Event field: {}\n(primary pick: [{}] {})\n",
            m.event_ticker, m.ticker, m.title
        ));
        // Include primary first for sum context
        block.push_str(&format!(
            "- [{}] {} — mid-implied {:.1}% (bid ${:.4} / ask ${:.4})  [selected]\n",
            m.ticker, m.title, m.yes_prob_pct, m.yes_bid, m.yes_ask
        ));
        let mut field_sum = m.yes_prob_pct;
        for s in &sibs {
            let gate = assess_market_gate_for_ticker(
                Some(&s.ticker),
                &s.status,
                &s.result,
                s.close_time.as_deref(),
                s.expiration_time.as_deref(),
                now,
            );
            if !gates.iter().any(|(t, _)| t.eq_ignore_ascii_case(&s.ticker)) {
                gates.push((s.ticker.clone(), gate.clone()));
            }
            field_sum += s.yes_prob_pct;
            block.push_str(&format!(
                "- [{}] {} — mid-implied {:.1}% (bid ${:.4} / ask ${:.4}), Spread {:.1}c, GATE={}\n",
                s.ticker,
                s.title,
                s.yes_prob_pct,
                s.yes_bid,
                s.yes_ask,
                s.spread * 100.0,
                gate.label()
            ));
        }
        block.push_str(&format!(
            "- Field sum (selected + siblings listed): ~{:.1}% (siblings mutually exclusive unless rules say otherwise)\n\n",
            field_sum
        ));
    }
    if !block.is_empty() {
        ctx.push_str("## SIBLING FIELD (same event_ticker — use for relative fair value)\n");
        ctx.push_str(
            "Do not treat a single mid-tier name as frontrunner without comparing this field. \
             YES pays only for the named outcome on each ticker.\n\n",
        );
        ctx.push_str(&block);
    }
}

/// Whether the user query warrants cross-asset (Fincept) spot levels.
pub fn needs_cross_asset_context(query: &str) -> bool {
    let q = query.to_lowercase();
    [
        "gold", "silver", "oil", "crude", "spy", "qqq", "s&p", "spx", "nasdaq", "vix",
        "bitcoin", "btc", "eth", "crypto", "yield", "treasury", "fed", "cpi", "inflation",
        "fx", "forex", "dollar", "eur", "macro", "commodity", "commodities", "rates",
        "equity", "equities", "stock", "index",
    ]
    .iter()
    .any(|k| q.contains(k))
}

/// Fetch top markets by category
async fn fetch_category_stats(client: &KalshiClient) -> Result<Vec<CategorySnapshot>, String> {
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
    fn cross_asset_gate_macro_vs_politics() {
        assert!(needs_cross_asset_context("What is gold doing vs CPI markets?"));
        assert!(!needs_cross_asset_context(
            "What are the most mispriced political markets on Kalshi today?"
        ));
    }

    #[test]
    fn test_non_sports_query_no_sports_injected() {
        // This test proves that sports data is not injected for non-sports queries
        let query = "Analyze the Federal Reserve decision market";
        assert!(!is_sports_market_query(query));
    }

    #[test]
    fn assess_chat_context_degraded_when_tape_empty() {
        use crate::kalshi::KalshiConfig;
        let client = KalshiClient::new(KalshiConfig::default(), None);
        let status = assess_kalshi_chat_context(&client);
        assert!(status.degraded);
        assert_eq!(status.tape_market_count, 0);
        assert!(!status.reasons.is_empty());
    }

    #[test]
    fn format_resolution_rules_preserves_short_text() {
        let rules = "YES resolves if Candidate A wins the primary.";
        assert_eq!(format_resolution_rules(rules, 2500), rules);
    }

    #[test]
    fn format_resolution_rules_truncates_long_text() {
        let rules = "word ".repeat(800);
        let out = format_resolution_rules(&rules, 200);
        assert!(out.len() < rules.len());
        assert!(out.contains("truncated"));
    }

    #[test]
    fn longshot_scan_flag() {
        assert!(wants_longshot_scan("show me longshot misprices"));
        assert!(!wants_longshot_scan("most mispriced markets today"));
    }

    #[test]
    fn retrieved_line_separates_ask_dollars_from_mid_pct() {
        use crate::kalshi::KalshiMarketSummary;
        let m = KalshiMarketSummary {
            ticker: "KXATP-NED".into(),
            event_ticker: "EV".into(),
            title: "Nedic wins".into(),
            category: "Sports".into(),
            status: "active".into(),
            yes_prob_pct: 48.0,
            yes_ask: 0.94,
            yes_bid: 0.02,
            no_ask: 0.98,
            no_bid: 0.06,
            last_price: 0.50,
            volume_24h: 0.0,
            total_volume: 0.0,
            liquidity: 0.0,
            spread: 0.92,
            close_time: Some("2028-01-01T00:00:00Z".into()),
            expiration_time: None,
            result: String::new(),
            can_close_early: false,
            is_provisional: false,
        };
        let line = format_retrieved_market_line(&m, "active", "—", &MarketGate::Open);
        // Must NOT look like "Yes: $0.9400 (48.00%)"
        assert!(!line.contains("Yes: $0.9400 (48"));
        assert!(line.contains("bid $0.0200"));
        assert!(line.contains("ask $0.9400"));
        assert!(line.contains("mid-implied 48.00%"));
        assert!(line.contains("WIDE_SPREAD"));
        assert!(line.contains("THIN/NO_VOLUME"));
    }

    #[test]
    fn retrieval_penalizes_wide_spread_zero_volume() {
        use crate::kalshi::KalshiMarketSummary;
        let tight = KalshiMarketSummary {
            ticker: "KX-TIGHT".into(),
            event_ticker: "EV".into(),
            title: "tight politics".into(),
            category: "Politics".into(),
            status: "open".into(),
            yes_prob_pct: 45.0,
            yes_ask: 0.46,
            yes_bid: 0.44,
            no_ask: 0.56,
            no_bid: 0.54,
            last_price: 0.45,
            volume_24h: 50_000.0,
            total_volume: 50_000.0,
            liquidity: 10_000.0,
            spread: 0.02,
            close_time: Some("2028-01-01T00:00:00Z".into()),
            expiration_time: None,
            result: String::new(),
            can_close_early: false,
            is_provisional: false,
        };
        let wide = KalshiMarketSummary {
            ticker: "KX-WIDE".into(),
            event_ticker: "EV".into(),
            title: "wide tennis".into(),
            category: "Sports".into(),
            status: "open".into(),
            yes_prob_pct: 48.0,
            yes_ask: 0.94,
            yes_bid: 0.02,
            no_ask: 0.98,
            no_bid: 0.06,
            last_price: 0.50,
            volume_24h: 0.0,
            total_volume: 0.0,
            liquidity: 0.0,
            spread: 0.92,
            close_time: Some("2028-01-01T00:00:00Z".into()),
            expiration_time: None,
            result: String::new(),
            can_close_early: false,
            is_provisional: false,
        };
        let now = chrono::Utc::now();
        let selected = select_markets_for_query(
            &[wide.clone(), tight.clone()],
            &[],
            None,
            2,
            false,
            now,
        );
        assert!(!selected.is_empty());
        assert_eq!(selected[0].ticker, "KX-TIGHT");
        // Wide zero-volume book should rank out of a default non-longshot scan
        assert!(!selected.iter().any(|m| m.ticker == "KX-WIDE"));
    }

    #[test]
    fn retrieval_prefers_mid_prob_over_extremes() {
        use crate::kalshi::KalshiMarketSummary;
        let mid = KalshiMarketSummary {
            ticker: "KX-MID".into(),
            event_ticker: "EV".into(),
            title: "mid politics".into(),
            category: "Politics".into(),
            status: "open".into(),
            yes_prob_pct: 45.0,
            yes_ask: 0.45,
            yes_bid: 0.44,
            no_ask: 0.56,
            no_bid: 0.55,
            last_price: 0.45,
            volume_24h: 10_000.0,
            total_volume: 10_000.0,
            liquidity: 5_000.0,
            spread: 0.01,
            close_time: Some("2028-01-01T00:00:00Z".into()),
            expiration_time: None,
            result: String::new(),
            can_close_early: false,
            is_provisional: false,
        };
        let extreme = KalshiMarketSummary {
            ticker: "KX-EXT".into(),
            title: "extreme politics".into(),
            yes_prob_pct: 0.4,
            yes_ask: 0.004,
            volume_24h: 200_000.0,
            liquidity: 0.0,
            spread: 0.0,
            ..mid.clone()
        };
        let now = chrono::Utc::now();
        let picked = select_markets_for_query(
            &[extreme.clone(), mid.clone()],
            &["politics".into()],
            None,
            2,
            false,
            now,
        );
        assert!(!picked.is_empty());
        assert_eq!(picked[0].ticker, "KX-MID");
    }
}
