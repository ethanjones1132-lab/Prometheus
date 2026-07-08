//! Fincept cross-asset context for the chat pipeline (plan §7 Phase 1).
//! Appends live world-market snapshot when the sidecar bridge is online.

use crate::fincept_bridge::FinceptBridge;
use serde_json::Value;

/// Append cross-asset market data from the Fincept sidecar when available.
pub async fn append_fincept_context(bridge: &FinceptBridge, ctx: &mut String) {
    let status = bridge.status().await;
    if !status.online {
        ctx.push_str("## CROSS-ASSET CONTEXT (Fincept)\n");
        if status.degraded {
            ctx.push_str("(Analysis engine degraded — Kalshi-only context. Restart the app or use Settings to retry the sidecar.)\n\n");
        } else {
            ctx.push_str("(Analysis engine offline — Kalshi-only context.)\n\n");
        }
        return;
    }

    match bridge.get_json("/api/v1/market/snapshot").await {
        Ok(json) => {
            ctx.push_str("## CROSS-ASSET CONTEXT (Fincept sidecar — live yfinance snapshot)\n");
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
        match row.get("last_price").and_then(|v| v.as_f64()) {
            Some(px) => {
                out.push_str(&format!(
                    "- [{category}] {ticker} ({name}): last={px:.4}\n"
                ));
            }
            None => {
                out.push_str(&format!(
                    "- [{category}] {ticker} ({name}): quote unavailable\n"
                ));
            }
        }
    }
    out.push_str(
        "Use these spot levels as macro context for index, rates, FX, and commodity-linked Kalshi contracts — not as trade signals.\n\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_snapshot_renders_prices() {
        let json: Value = serde_json::json!({
            "instruments": [
                {"ticker": "SPY", "name": "S&P 500", "category": "stocks", "last_price": 501.25},
                {"ticker": "BTC-USD", "name": "Bitcoin", "category": "crypto", "last_price": null}
            ]
        });
        let s = format_snapshot_for_prompt(&json);
        assert!(s.contains("SPY"));
        assert!(s.contains("501.2500"));
        assert!(s.contains("quote unavailable"));
    }
}