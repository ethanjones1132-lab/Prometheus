//! Fincept cross-asset context for the chat pipeline (plan §7 Phase 1).
//! Appends live world-market snapshot when the sidecar bridge is online
//! **and** the user query warrants macro/spot levels (query-gated).

use crate::chat::kalshi_context::needs_cross_asset_context;
use crate::fincept_bridge::FinceptBridge;
use serde_json::Value;

/// Append cross-asset market data from the Fincept sidecar when available.
/// Skips the snapshot (and the offline noise) when the query does not need it.
pub async fn append_fincept_context(bridge: &FinceptBridge, ctx: &mut String) {
    append_fincept_context_for_query(bridge, ctx, "").await;
}

/// Query-aware entry used by the chat pipeline.
pub async fn append_fincept_context_for_query(
    bridge: &FinceptBridge,
    ctx: &mut String,
    user_message: &str,
) {
    if !user_message.is_empty() && !needs_cross_asset_context(user_message) {
        return;
    }

    let status = bridge.status().await;
    if !status.online {
        // Only mention offline when the query actually needed spots
        if needs_cross_asset_context(user_message) || user_message.is_empty() {
            ctx.push_str("## CROSS-ASSET CONTEXT (Fincept)\n");
            if status.degraded {
                ctx.push_str("(Analysis engine degraded — no live spots. Restart sidecar if needed.)\n\n");
            } else {
                ctx.push_str("(Analysis engine offline — no live spots.)\n\n");
            }
        }
        return;
    }

    match bridge.get_json("/api/v1/market/snapshot").await {
        Ok(json) => {
            ctx.push_str("## CROSS-ASSET CONTEXT (Fincept sidecar — yfinance)\n");
            ctx.push_str(
                "Cite ONLY these printed last prices. Values are instrument last prints \
                 (futures/ETF/FX as labeled), not Kalshi contract mids.\n",
            );
            ctx.push_str(&format_snapshot_for_prompt(&json));
            ctx.push('\n');
        }
        Err(e) => {
            ctx.push_str("## CROSS-ASSET CONTEXT (Fincept)\n");
            ctx.push_str(&format!("(Snapshot unavailable: {e})\n\n"));
        }
    }
}

fn format_snapshot_for_prompt(json: &Value) -> String {
    let mut out = String::new();
    let Some(instruments) = json.get("instruments").and_then(|v| v.as_array()) else {
        return "(empty snapshot)\n\n".into();
    };
    for row in instruments {
        let ticker = row.get("ticker").and_then(|v| v.as_str()).unwrap_or("?");
        let name = row.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let category = row.get("category").and_then(|v| v.as_str()).unwrap_or("");
        let fetched = row
            .get("fetched_at")
            .and_then(|v| v.as_f64())
            .map(|ts| format!(" as_of_unix={ts:.0}"))
            .unwrap_or_default();
        let source = row
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("yfinance");
        match row.get("last_price").and_then(|v| v.as_f64()) {
            Some(px) => {
                let label = instrument_label(ticker, name);
                let note = price_sanity_note(ticker, px);
                out.push_str(&format!(
                    "- [{category}] {ticker} ({label}): last={px:.4} source={source}{fetched}{note}\n"
                ));
            }
            None => {
                out.push_str(&format!(
                    "- [{category}] {ticker} ({name}): quote unavailable source={source}\n"
                ));
            }
        }
    }
    out.push_str(
        "Use as macro context for linked Kalshi contracts only when relevant. Not trade signals.\n\n",
    );
    out
}

fn instrument_label(ticker: &str, name: &str) -> String {
    match ticker {
        "GC=F" => "Gold futures front".into(),
        "CL=F" => "WTI crude futures front".into(),
        "^VIX" => "VIX index".into(),
        "^TNX" => "US 10Y yield %".into(),
        "SPY" => "S&P 500 ETF".into(),
        "QQQ" => "Nasdaq-100 ETF".into(),
        "BTC-USD" => "Bitcoin USD".into(),
        _ if !name.is_empty() => name.to_string(),
        _ => ticker.to_string(),
    }
}

fn price_sanity_note(ticker: &str, px: f64) -> &'static str {
    match ticker {
        // Gold futures historically ~$1k–$5k/oz in modern era; flag extremes
        "GC=F" if !(800.0..=6000.0).contains(&px) => " ⚠ unusual gold print — verify",
        "CL=F" if !(10.0..=300.0).contains(&px) => " ⚠ unusual oil print — verify",
        "SPY" if !(50.0..=1000.0).contains(&px) => " ⚠ unusual SPY print — verify",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_snapshot_renders_prices() {
        let json: Value = serde_json::json!({
            "instruments": [
                {"ticker": "SPY", "name": "S&P 500", "category": "stocks", "last_price": 501.25, "source": "yfinance", "fetched_at": 1.0},
                {"ticker": "BTC-USD", "name": "Bitcoin", "category": "crypto", "last_price": null}
            ]
        });
        let s = format_snapshot_for_prompt(&json);
        assert!(s.contains("SPY"));
        assert!(s.contains("501.2500"));
        assert!(s.contains("quote unavailable"));
        assert!(s.contains("source=yfinance"));
    }

    #[test]
    fn gold_label_is_futures() {
        assert!(instrument_label("GC=F", "Gold").contains("futures"));
    }
}