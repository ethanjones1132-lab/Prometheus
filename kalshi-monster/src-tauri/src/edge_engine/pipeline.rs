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

/// Compact web evidence row for the news agent (Rust → sidecar context).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebSnippet {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Sidecar depth tier (Sprint 3 / plan §7).
///
/// - **Quick** — board scan: contract_tape only, no yfinance / news fetch
/// - **Standard** — default Analyze / chat priors: technical + tape + news if snippets
/// - **Deep** — manual deep: full agents + fresh history path + web snippets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisDepth {
    Quick,
    #[default]
    Standard,
    Deep,
}

impl AnalysisDepth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Standard => "standard",
            Self::Deep => "deep",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "quick" => Self::Quick,
            "deep" => Self::Deep,
            _ => Self::Standard,
        }
    }
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
    /// Days until close (from tape); technical prefers this over re-parsing close_time.
    pub horizon_days: Option<f64>,
    /// Structured snippets for news agent — never invents p without these.
    pub web_snippets: Vec<WebSnippet>,
    /// Agent fan-out tier (quick / standard / deep).
    pub depth: AnalysisDepth,
    /// Optional FRED key for macro agent (from OS secrets / env).
    pub fred_api_key: Option<String>,
    pub flags: Vec<String>,
}

/// Absolute edge score for ranking: max(|edge_yes|, |edge_no|).
pub fn abs_edge_net_score(edge_net_yes: f64, edge_net_no: f64) -> f64 {
    edge_net_yes.abs().max(edge_net_no.abs())
}

/// Rank analysis results by |edge_net| descending (stable for equal scores).
pub fn rank_by_abs_edge_net(results: &[EdgeAnalysisResult]) -> Vec<EdgeAnalysisResult> {
    let mut ranked = results.to_vec();
    ranked.sort_by(|a, b| {
        let sa = abs_edge_net_score(a.edge_net_yes, a.edge_net_no);
        let sb = abs_edge_net_score(b.edge_net_yes, b.edge_net_no);
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.market_ticker.cmp(&b.market_ticker))
    });
    ranked
}

/// Routing weight mass sitting on agents that returned `probability=None`.
/// Returns (silent_fraction, silent_agent_names) relative to total routing table mass.
pub fn silent_agent_weight_report(
    signals: &[AgentSignal],
    weights: &HashMap<String, f64>,
) -> Option<(f64, Vec<String>)> {
    let total: f64 = weights.values().copied().sum();
    if total <= 0.0 {
        return None;
    }
    let mut silent_mass = 0.0;
    let mut silent_names: Vec<String> = Vec::new();
    for (agent, w) in weights {
        if *w <= 0.0 {
            continue;
        }
        let opined = signals
            .iter()
            .any(|s| s.agent == *agent && s.probability.is_some());
        if !opined {
            silent_mass += *w;
            silent_names.push(agent.clone());
        }
    }
    if silent_names.is_empty() {
        return None;
    }
    silent_names.sort();
    Some((silent_mass / total, silent_names))
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
    if let Some(h) = input.horizon_days {
        context.insert("horizon_days".into(), serde_json::json!(h));
    }
    if !input.web_snippets.is_empty() {
        context.insert(
            "web_snippets".into(),
            serde_json::json!(input.web_snippets),
        );
    }
    if let Some(ref key) = input.fred_api_key {
        if !key.is_empty() {
            context.insert("fred_api_key".into(), serde_json::json!(key));
        }
    }
    // Depth hint for sidecar orchestrator (quick/standard/deep).
    context.insert("depth".into(), serde_json::json!(input.depth.as_str()));
    flags.push(format!("depth:{}", input.depth.as_str()));

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
            let opining = signals.iter().filter(|s| s.probability.is_some()).count() as u64;
            bridge
                .record_agent_call(
                    sidecar_elapsed_ms,
                    signals.len() as u64,
                    opining,
                    None,
                )
                .await;
        }
        Err(e) => {
            tracing::warn!("sidecar market-opinion failed: {e}");
            flags.push(format!("sidecar_unavailable: {e}"));
            bridge
                .record_agent_call(None, 0, 0, Some(e.clone()))
                .await;
        }
    }

    let weights = routing_weights_for_category(&input.category);
    let opinion: Option<ModelOpinion> = aggregate(&signals, &weights);
    let signals_opining = signals.iter().filter(|s| s.probability.is_some()).count();

    // Sprint 1.3 / 4.3: surface routing weight on silent agents (macro/news often null).
    // Lower threshold for economic categories so a silent 50% macro always shows.
    let silent_threshold =
        if sidecar_category(&input.category) == "economic" {
            0.15
        } else {
            0.25
        };
    let silent_note = silent_agent_weight_report(&signals, &weights).and_then(|(frac, names)| {
        if frac < silent_threshold {
            return None;
        }
        let macro_note = if names.iter().any(|n| n == "macro")
            && sidecar_category(&input.category) == "economic"
        {
            " macro is 50% of economic routing when mapped FRED data is absent."
        } else {
            ""
        };
        Some(format!(
            "silent agent weight {:.0}% of routing ({}) — p_model thin when only {} of {} agents opine.{}",
            frac * 100.0,
            names.join(", "),
            signals_opining,
            signals.len(),
            macro_note
        ))
    });

    let (p_model, mut edge): (Option<f64>, EdgeVerdict) = if let Some(ref op) = opinion {
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
    if let Some(note) = silent_note {
        edge.reasons.push(note);
    }

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
        crate::kalshi::forecast::ForecastProvenance {
            source: "app",
            agents_opining: Some(signals_opining as i64),
        },
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

    #[test]
    fn rank_by_abs_edge_puts_largest_first() {
        let mk = |ticker: &str, yes: f64, no: f64| EdgeAnalysisResult {
            forecast_id: 1,
            market_ticker: ticker.into(),
            p_market: 0.5,
            p_model: None,
            p_final: 0.5,
            confidence: 0.0,
            verdict: "pass".into(),
            verdict_reasons: vec![],
            agent_breakdown: None,
            edge_net_yes: yes,
            edge_net_no: no,
            signals_received: 0,
            signals_opining: 0,
            sidecar_elapsed_ms: None,
        };
        let ranked = rank_by_abs_edge_net(&[
            mk("A", 0.01, 0.0),
            mk("B", 0.0, -0.08),
            mk("C", 0.03, 0.02),
        ]);
        assert_eq!(ranked[0].market_ticker, "B");
        assert_eq!(ranked[1].market_ticker, "C");
        assert_eq!(ranked[2].market_ticker, "A");
    }

    #[test]
    fn analysis_depth_parse_and_str() {
        assert_eq!(AnalysisDepth::parse("quick"), AnalysisDepth::Quick);
        assert_eq!(AnalysisDepth::parse("DEEP"), AnalysisDepth::Deep);
        assert_eq!(AnalysisDepth::parse("whatever"), AnalysisDepth::Standard);
        assert_eq!(AnalysisDepth::Quick.as_str(), "quick");
        assert_eq!(AnalysisDepth::Standard.as_str(), "standard");
        assert_eq!(AnalysisDepth::Deep.as_str(), "deep");
    }

    #[test]
    fn silent_weight_reports_macro_news_mass() {
        let mut weights = HashMap::new();
        weights.insert("macro".into(), 0.50);
        weights.insert("news".into(), 0.20);
        weights.insert("technical".into(), 0.20);
        weights.insert("contract_tape".into(), 0.10);
        let signals = vec![
            AgentSignal {
                agent: "technical".into(),
                probability: Some(0.55),
                confidence: 0.4,
                rationale: "ok".into(),
                inputs_used: vec![],
                caveats: vec![],
            },
            AgentSignal {
                agent: "macro".into(),
                probability: None,
                confidence: 0.0,
                rationale: "no data".into(),
                inputs_used: vec![],
                caveats: vec![],
            },
            AgentSignal {
                agent: "news".into(),
                probability: None,
                confidence: 0.0,
                rationale: "no snippets".into(),
                inputs_used: vec![],
                caveats: vec![],
            },
        ];
        let (frac, names) = silent_agent_weight_report(&signals, &weights).unwrap();
        assert!(frac > 0.5, "frac={frac}");
        assert!(names.contains(&"macro".to_string()));
        assert!(names.contains(&"news".to_string()));
    }
}
