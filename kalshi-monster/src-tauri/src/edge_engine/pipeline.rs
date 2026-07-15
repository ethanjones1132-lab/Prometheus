//! End-to-end edge pipeline: sidecar signals → aggregate → evaluate → forecast ledger.
//!
//! Keeps AGPL isolation: only HTTP JSON crosses the process boundary. No Python
//! imports in Rust.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};

use super::{
    aggregate, evaluate, AgentSignal, EdgeConfig, EdgeVerdict, ModelOpinion, Quote, Verdict,
};
use crate::fincept_bridge::FinceptBridge;

/// Plan §5.2 routing weights (config, not hard-coded forever).
pub fn routing_weights_for_category(category: &str) -> HashMap<String, f64> {
    let c = category.to_ascii_lowercase();
    let pairs: &[(&str, f64)] = if c.contains("economic") || c.contains("econ") {
        &[
            ("macro", 0.50),
            ("news", 0.20),
            ("technical", 0.20),
            ("contract_tape", 0.20),
            ("sentiment", 0.10),
        ]
    } else if c.contains("index")
        || c.contains("price")
        || c.contains("crypto")
        || c.contains("financ") // finance / financials
        || c.contains("stock")
    {
        &[
            ("technical", 0.45),
            ("macro", 0.25),
            ("news", 0.15),
            ("sentiment", 0.15),
            ("contract_tape", 0.20),
        ]
    } else if c.contains("company") || c.contains("earnings") || c.contains("equity") {
        &[
            ("fundamentals", 0.30),
            ("valuation", 0.25),
            ("news", 0.25),
            ("sentiment", 0.10),
            ("technical", 0.10),
            ("contract_tape", 0.10),
        ]
    } else if c.contains("politic") || c.contains("election") {
        &[
            ("news", 0.35),
            ("sentiment", 0.30),
            ("macro", 0.15),
            ("technical", 0.10),
            ("contract_tape", 0.20),
        ]
    } else {
        // Weather / science / other + contract-tape secondary
        &[
            ("news", 0.50),
            ("technical", 0.20),
            ("contract_tape", 0.30),
            ("sentiment", 0.20),
        ]
    };
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), *v))
        .collect()
}

/// Map app / Kalshi category strings to sidecar `MarketCategory` enum values.
pub fn sidecar_category(category: &str) -> &'static str {
    let c = category.to_ascii_lowercase();
    if c.contains("economic") || c.contains("econ") || c.contains("fed") || c.contains("cpi") {
        "economic"
    } else if c.contains("politic") || c.contains("election") {
        "political"
    } else if c.contains("company") || c.contains("earnings") {
        "company_event"
    } else if c.contains("index")
        || c.contains("financ") // finance / financials
        || c.contains("crypto")
        || c.contains("stock")
        || c.contains("price")
    {
        "index_price_level"
    } else {
        "other"
    }
}

fn verdict_str(v: Verdict) -> &'static str {
    match v {
        Verdict::TradeYes => "trade_yes",
        Verdict::TradeNo => "trade_no",
        Verdict::Pass => "pass",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeAnalysisResult {
    pub forecast_id: i64,
    pub market_ticker: String,
    pub p_market: f64,
    pub p_model: Option<f64>,
    pub p_final: f64,
    pub confidence: f64,
    pub verdict: String,
    pub verdict_reasons: Vec<String>,
    pub agent_breakdown: Option<serde_json::Value>,
    pub edge_net_yes: f64,
    pub edge_net_no: f64,
    pub signals_received: usize,
    pub signals_opining: usize,
    pub sidecar_elapsed_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AnalyzeMarketInput {
    pub market_ticker: String,
    pub title: String,
    pub resolution_rules: String,
    pub close_time: String,
    pub category: String,
    pub yes_bid: f64,
    pub yes_ask: f64,
    /// Optional mid history from price_tracker (0–1 YES mids).
    pub contract_mids: Vec<f64>,
    pub underlying_ticker: Option<String>,
    pub strike: Option<f64>,
    pub flags: Vec<String>,
}

/// Call sidecar agents (if online), aggregate, evaluate, insert forecast ledger row.
///
/// When the sidecar is offline or no agent opines, still writes a market-only
/// row (`p_model=None`, `p_final=p_market`, verdict usually pass) so the ledger
/// keeps receiving *real* market observations without inventing model edge.
pub async fn analyze_and_log_forecast(
    pool: &Pool<Sqlite>,
    bridge: &FinceptBridge,
    input: AnalyzeMarketInput,
    cfg: &EdgeConfig,
) -> Result<EdgeAnalysisResult, String> {
    let quote = Quote {
        yes_bid: input.yes_bid.clamp(0.0, 1.0),
        yes_ask: input.yes_ask.clamp(0.0, 1.0),
    };
    let p_market = quote.mid().clamp(0.01, 0.99);

    let mut signals: Vec<AgentSignal> = Vec::new();
    let mut sidecar_elapsed_ms: Option<i64> = None;
    let mut flags = input.flags.clone();

    let mut context = serde_json::Map::new();
    if !input.contract_mids.is_empty() {
        context.insert(
            "contract_mids".into(),
            serde_json::json!(input.contract_mids),
        );
    }
    if let Some(t) = &input.underlying_ticker {
        context.insert("underlying_ticker".into(), serde_json::json!(t));
    }
    if let Some(k) = input.strike {
        context.insert("strike".into(), serde_json::json!(k));
    }
    // Depth hint for sidecar orchestrator (quick/standard/deep); agents may ignore.
    context.insert("depth".into(), serde_json::json!("standard"));

    let body = serde_json::json!({
        "market_ticker": input.market_ticker,
        "title": input.title,
        "resolution_rules": if input.resolution_rules.is_empty() {
            input.title.clone()
        } else {
            input.resolution_rules.clone()
        },
        "close_time": input.close_time,
        "category": sidecar_category(&input.category),
        "yes_bid": quote.yes_bid,
        "yes_ask": quote.yes_ask,
        "context": context,
    });

    match bridge.post_json("/api/v1/agents/market-opinion", &body).await {
        Ok(resp) => {
            sidecar_elapsed_ms = resp.get("elapsed_ms").and_then(|v| v.as_i64());
            if let Some(arr) = resp.get("signals").and_then(|s| s.as_array()) {
                for s in arr {
                    match serde_json::from_value::<AgentSignal>(s.clone()) {
                        Ok(sig) => signals.push(sig),
                        Err(e) => {
                            tracing::warn!("skip malformed AgentSignal: {e}");
                        }
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("sidecar market-opinion failed: {e}");
            flags.push(format!("sidecar_unavailable: {e}"));
        }
    }

    let weights = routing_weights_for_category(&input.category);
    let opinion: Option<ModelOpinion> = aggregate(&signals, &weights);
    let signals_opining = signals.iter().filter(|s| s.probability.is_some()).count();

    let (p_model, edge): (Option<f64>, EdgeVerdict) = if let Some(ref op) = opinion {
        let v = evaluate(op, quote, &flags, cfg);
        (Some(op.p_model), v)
    } else {
        // Market-only ledger row — honest Phase-0 style evidence.
        let mut reasons = vec![
            "no agent opinion available; p_final set to p_market (no fabricated model edge)"
                .to_string(),
        ];
        reasons.extend(flags.iter().cloned());
        let v = EdgeVerdict {
            p_market,
            p_model: p_market,
            p_final: p_market,
            confidence: 0.0,
            entry_yes: super::entry_cost(quote.yes_ask, cfg.fee_multiplier),
            entry_no: super::entry_cost(quote.no_ask(), cfg.fee_multiplier),
            edge_net_yes: 0.0,
            edge_net_no: 0.0,
            verdict: Verdict::Pass,
            reasons,
        };
        (None, v)
    };

    let agent_breakdown = if signals.is_empty() {
        None
    } else {
        Some(serde_json::json!({
            "signals": signals,
            "contributions": opinion.as_ref().map(|o| &o.contributions),
            "weights": weights,
        }))
    };

    let reasons_json = serde_json::to_string(&edge.reasons).unwrap_or_else(|_| "[]".into());
    let breakdown_str = agent_breakdown
        .as_ref()
        .map(|v| v.to_string());

    let now = chrono::Utc::now().to_rfc3339();
    let forecast_id = crate::kalshi::forecast::insert_forecast(
        pool,
        &input.market_ticker,
        &now,
        &input.close_time,
        edge.p_market,
        p_model,
        edge.p_final,
        verdict_str(edge.verdict),
        &reasons_json,
        None,
        breakdown_str.as_deref(),
    )
    .await?;

    Ok(EdgeAnalysisResult {
        forecast_id,
        market_ticker: input.market_ticker,
        p_market: edge.p_market,
        p_model,
        p_final: edge.p_final,
        confidence: edge.confidence,
        verdict: verdict_str(edge.verdict).to_string(),
        verdict_reasons: edge.reasons,
        agent_breakdown,
        edge_net_yes: edge.edge_net_yes,
        edge_net_no: edge.edge_net_no,
        signals_received: signals.len(),
        signals_opining,
        sidecar_elapsed_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_index_includes_technical() {
        let w = routing_weights_for_category("Financials");
        assert!(w.get("technical").copied().unwrap_or(0.0) > 0.0);
    }

    #[test]
    fn sidecar_category_maps_finance() {
        assert_eq!(sidecar_category("Financials"), "index_price_level");
        assert_eq!(sidecar_category("Politics"), "political");
    }
}
