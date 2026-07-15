//! Shared builder for sidecar `market-opinion` / edge analyze inputs (Phase A).
//!
//! Used by both Command-desk Analyze and Analyst chat so contract mids,
//! underlying/strike inference, and tape fields stay consistent.

use sqlx::{Pool, Sqlite};

use super::pipeline::AnalyzeMarketInput;
use crate::kalshi::models::KalshiMarket;
use crate::kalshi::price_tracker;
use crate::kalshi::KalshiClient;

/// Max mid history points sent to the contract_tape agent.
pub const DEFAULT_MIDS_LIMIT: i64 = 50;

/// Infer yfinance-style underlying + strike from ticker/title/rules.
/// Best-effort only — agents still return probability=None when insufficient.
pub fn infer_underlying_and_strike(
    market_ticker: &str,
    title: &str,
    rules: &str,
) -> (Option<String>, Option<f64>) {
    let hay = format!("{market_ticker} {title} {rules}").to_uppercase();
    let underlying = infer_underlying_ticker(&hay, market_ticker);
    let strike = infer_strike_from_text(&format!("{title} {rules}"));
    (underlying, strike)
}

fn infer_underlying_ticker(hay: &str, market_ticker: &str) -> Option<String> {
    // Series prefixes first (Kalshi naming).
    let t = market_ticker.to_uppercase();
    if t.starts_with("KXBTCD") || t.starts_with("KXBTC") || t.contains("BITCOIN") {
        return Some("BTC-USD".into());
    }
    if t.starts_with("KXETHD") || t.starts_with("KXETH") || t.contains("ETHEREUM") {
        return Some("ETH-USD".into());
    }
    if t.contains("INX") || t.contains("SPX") || t.contains("SPY") {
        return Some("^GSPC".into());
    }
    if t.contains("NDX") || t.contains("NASDAQ") || t.contains("QQQ") {
        return Some("^NDX".into());
    }

    // Token map — longer keys first.
    let pairs: &[(&str, &str)] = &[
        ("S&P 500", "^GSPC"),
        ("S&P500", "^GSPC"),
        ("NASDAQ-100", "^NDX"),
        ("BITCOIN", "BTC-USD"),
        ("ETHEREUM", "ETH-USD"),
        ("RUSSELL", "^RUT"),
        ("NASDAQ", "^IXIC"),
        ("GOLD", "GC=F"),
        ("WTI", "CL=F"),
        ("OIL", "CL=F"),
        ("SPX", "^GSPC"),
        ("SPY", "SPY"),
        ("S&P", "^GSPC"),
        ("NDX", "^NDX"),
        ("QQQ", "QQQ"),
        ("RUT", "^RUT"),
        ("IWM", "IWM"),
        ("BTC", "BTC-USD"),
        ("ETH", "ETH-USD"),
        ("AAPL", "AAPL"),
        ("TSLA", "TSLA"),
        ("MSFT", "MSFT"),
        ("NVDA", "NVDA"),
        ("AMZN", "AMZN"),
        ("GOOGL", "GOOGL"),
        ("META", "META"),
    ];
    let mut best: Option<(usize, &str)> = None;
    for (token, yf) in pairs {
        if let Some(idx) = hay.find(&token.to_uppercase()) {
            let score = token.len() * 1000 + (1000 - idx.min(999));
            if best.map(|(s, _)| score > s).unwrap_or(true) {
                best = Some((score, yf));
            }
        }
    }
    best.map(|(_, yf)| yf.to_string())
}

fn infer_strike_from_text(text: &str) -> Option<f64> {
    // Number token: 5500 | 5,500 | 100,000 | 100000.5
    let num = r"([0-9]{1,3}(?:,[0-9]{3})+|[0-9]+(?:\.[0-9]+)?)";
    let patterns = [
        format!(r"(?i)(?:above|over|exceed(?:s|ing)?|greater than|>\s*)\$?\s*{num}"),
        format!(r"(?i)(?:close\s+(?:at|above)|settle\s+above)\s+\$?\s*{num}"),
        format!(r"(?i)\$\s*{num}"),
    ];
    for pat in patterns {
        if let Ok(re) = regex::Regex::new(&pat) {
            if let Some(caps) = re.captures(text) {
                if let Some(m) = caps.get(1) {
                    let cleaned = m.as_str().replace(',', "");
                    if let Ok(v) = cleaned.parse::<f64>() {
                        if v > 0.0 {
                            return Some(v);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Load YES mid history (0–1) from local price snapshots (most recent `limit` points, chronological).
pub async fn load_contract_mids(
    pool: &Pool<Sqlite>,
    ticker: &str,
    limit: i64,
) -> Vec<f64> {
    // Fetch newest-first then reverse so contract_tape sees time order.
    let rows = sqlx::query(
        r#"
        SELECT yes_prob_pct FROM kalshi_price_snapshots
        WHERE ticker = ?1
        ORDER BY snapshot_at DESC
        LIMIT ?2
        "#,
    )
    .bind(ticker)
    .bind(limit.max(2))
    .fetch_all(pool)
    .await;

    match rows {
        Ok(rows) => {
            let mut mids: Vec<f64> = rows
                .iter()
                .map(|r| {
                    let pct: f64 = sqlx::Row::get(r, "yes_prob_pct");
                    (pct / 100.0).clamp(0.01, 0.99)
                })
                .collect();
            mids.reverse();
            // Fallback: if no snapshots, use get_price_history helper path (empty).
            if mids.is_empty() {
                let history = price_tracker::get_price_history(pool, ticker, limit)
                    .await
                    .ok();
                if let Some(h) = history {
                    return h
                        .snapshots
                        .iter()
                        .map(|s| (s.yes_prob_pct / 100.0).clamp(0.01, 0.99))
                        .collect();
                }
            }
            mids
        }
        Err(_) => Vec::new(),
    }
}

/// Build analyze input from a full market row + optional mids (already loaded).
pub fn analyze_input_from_market(
    market: &KalshiMarket,
    contract_mids: Vec<f64>,
    flags: Vec<String>,
) -> AnalyzeMarketInput {
    let title = market.display_title();
    let rules = market.rules_primary.clone();
    let category = market
        .category
        .clone()
        .unwrap_or_else(|| market.infer_category().to_string());
    let close_time = market
        .close_time
        .clone()
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let (underlying_ticker, strike) =
        infer_underlying_and_strike(&market.ticker, &title, &rules);

    let mut flags = flags;
    if contract_mids.is_empty() {
        flags.push("no_contract_mids".into());
    }
    if underlying_ticker.is_some() {
        flags.push("underlying_inferred".into());
    }

    AnalyzeMarketInput {
        market_ticker: market.ticker.clone(),
        title,
        resolution_rules: rules,
        close_time,
        category,
        yes_bid: market.yes_bid(),
        yes_ask: market.yes_ask(),
        contract_mids,
        underlying_ticker,
        strike,
        flags,
    }
}

/// Resolve market from cache or live fetch, load mids, build input.
pub async fn build_analyze_input(
    client: &KalshiClient,
    pool: &Pool<Sqlite>,
    ticker: &str,
) -> Result<AnalyzeMarketInput, String> {
    let market = if let Some(m) = client.find_cached_market(ticker) {
        m
    } else {
        client.fetch_market(ticker).await?
    };
    let mids = load_contract_mids(pool, &market.ticker, DEFAULT_MIDS_LIMIT).await;
    Ok(analyze_input_from_market(&market, mids, vec![]))
}

/// Compact prompt lines for Analyst (not full JSON dump).
pub fn format_edge_result_for_prompt(
    r: &super::pipeline::EdgeAnalysisResult,
) -> String {
    let p_model = r
        .p_model
        .map(|p| format!("{p:.3}"))
        .unwrap_or_else(|| "null".into());
    let mut agents = String::new();
    if let Some(ref bd) = r.agent_breakdown {
        if let Some(sigs) = bd.get("signals").and_then(|s| s.as_array()) {
            let parts: Vec<String> = sigs
                .iter()
                .filter_map(|s| {
                    let name = s.get("agent")?.as_str()?;
                    let conf = s.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.0);
                    match s.get("probability").and_then(|p| p.as_f64()) {
                        Some(p) => Some(format!("{name}={p:.2}@{conf:.2}")),
                        None => Some(format!("{name}=null")),
                    }
                })
                .collect();
            agents = parts.join(", ");
        }
    }
    if agents.is_empty() {
        agents = "none".into();
    }
    format!(
        "- [{}] p_market={:.3} p_model={} p_final={:.3} conf={:.2} verdict={} opining={}/{} edge_yes={:.3} edge_no={:.3}\n  agents: {}\n",
        r.market_ticker,
        r.p_market,
        p_model,
        r.p_final,
        r.confidence,
        r.verdict,
        r.signals_opining,
        r.signals_received,
        r.edge_net_yes,
        r.edge_net_no,
        agents,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_btc_from_ticker() {
        let (u, _) = infer_underlying_and_strike("KXBTCD-26JUL15-B100000", "Bitcoin above", "");
        assert_eq!(u.as_deref(), Some("BTC-USD"));
    }

    #[test]
    fn infers_spy_from_title() {
        let (u, k) = infer_underlying_and_strike(
            "KXINX-26",
            "Will the S&P 500 close above 5500?",
            "Resolves yes if SPX > 5500",
        );
        assert_eq!(u.as_deref(), Some("^GSPC"));
        assert!(k.is_some());
        assert!((k.unwrap() - 5500.0).abs() < 1.0);
    }

    #[test]
    fn strike_from_above_pattern() {
        let k = infer_strike_from_text("Will Bitcoin be above $100,000 on July 15?");
        assert!(k.is_some());
        assert!((k.unwrap() - 100_000.0).abs() < 1.0);
    }

    #[test]
    fn format_edge_result_includes_ticker() {
        let r = super::super::pipeline::EdgeAnalysisResult {
            forecast_id: 1,
            market_ticker: "KXTEST-1".into(),
            p_market: 0.5,
            p_model: Some(0.55),
            p_final: 0.52,
            confidence: 0.3,
            verdict: "pass".into(),
            verdict_reasons: vec![],
            agent_breakdown: Some(serde_json::json!({
                "signals": [
                    {"agent": "technical", "probability": 0.55, "confidence": 0.4},
                    {"agent": "macro", "probability": null, "confidence": 0.0}
                ]
            })),
            edge_net_yes: 0.01,
            edge_net_no: -0.01,
            signals_received: 2,
            signals_opining: 1,
            sidecar_elapsed_ms: Some(12),
        };
        let s = format_edge_result_for_prompt(&r);
        assert!(s.contains("KXTEST-1"));
        assert!(s.contains("technical=0.55"));
        assert!(s.contains("macro=null"));
    }
}
