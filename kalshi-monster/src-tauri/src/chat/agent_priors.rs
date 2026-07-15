//! Sidecar agent priors for Analyst chat (Phase A of Fincept deep integration).
//!
//! Runs the same edge pipeline as Command-desk Analyze for a small set of open
//! retrieved markets, then injects a compact prior block into the system prompt.
//! Latency-bounded: short overall timeout, degrades cleanly when offline.

use std::time::Duration;

use sqlx::{Pool, Sqlite};

use crate::edge_engine::opinion_input::{self, format_edge_result_for_prompt};
use crate::edge_engine::pipeline::{analyze_and_log_forecast, EdgeAnalysisResult};
use crate::edge_engine::EdgeConfig;
use crate::fincept_bridge::FinceptBridge;
use crate::kalshi::KalshiClient;

/// Max open markets to run agents on per chat turn (latency budget).
pub const MAX_PRIOR_MARKETS: usize = 3;
/// Soft timeout for the whole prior batch before chat proceeds to the LLM.
pub const PRIOR_BATCH_TIMEOUT: Duration = Duration::from_secs(8);

/// Run edge pipeline on up to `MAX_PRIOR_MARKETS` tickers and return results.
pub async fn collect_sidecar_priors(
    client: &KalshiClient,
    bridge: &FinceptBridge,
    pool: &Pool<Sqlite>,
    cfg: &EdgeConfig,
    open_markets: &[(String, String)],
) -> Vec<EdgeAnalysisResult> {
    let mut out = Vec::new();
    for (ticker, _) in open_markets.iter().take(MAX_PRIOR_MARKETS) {
        match opinion_input::build_analyze_input(client, pool, ticker).await {
            Ok(input) => match analyze_and_log_forecast(pool, bridge, input, cfg).await {
                Ok(r) => out.push(r),
                Err(e) => tracing::warn!("chat agent prior {ticker}: {e}"),
            },
            Err(e) => tracing::warn!("chat prior input {ticker}: {e}"),
        }
    }
    out
}

/// Format priors for the LLM system prompt.
pub fn format_priors_prompt_block(
    results: &[EdgeAnalysisResult],
    bridge_online: bool,
    attempted: usize,
) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str("## SIDECAR MODEL PRIORS (Fincept agents — not final Kelly)\n");
    out.push_str(
        "These come from the local analysis sidecar (technical + contract_tape when data exists). \
         Use as a **prior** when forming fair_probability_pct. Do not ignore SETTLEMENT GATES or resolution rules. \
         Prefer PASS/WATCH when agents are silent or disagree sharply with a thin book.\n",
    );
    if !bridge_online {
        out.push_str("(Sidecar offline — no agent priors this turn.)\n\n");
        return out;
    }
    if results.is_empty() {
        out.push_str(&format!(
            "(No agent opinions this turn; attempted {attempted} open market(s).)\n\n"
        ));
        return out;
    }
    for r in results {
        out.push_str(&format_edge_result_for_prompt(r));
    }
    out.push_str(
        "If you TAKE and your fair diverges >10pts from p_final above, say why in thesis (ModelDisagreement).\n\n",
    );
    out
}

/// Append sidecar priors for open markets into `ctx` (best-effort, timed).
pub async fn append_agent_priors_for_chat(
    ctx: &mut String,
    client: &KalshiClient,
    bridge: &FinceptBridge,
    pool: &Pool<Sqlite>,
    cfg: &EdgeConfig,
    open_markets: &[(String, String)],
) {
    let status = bridge.status().await;
    let attempted = open_markets.len().min(MAX_PRIOR_MARKETS);

    if !status.online {
        ctx.push_str(&format_priors_prompt_block(&[], false, attempted));
        return;
    }

    let collect = collect_sidecar_priors(client, bridge, pool, cfg, open_markets);
    let results = match tokio::time::timeout(PRIOR_BATCH_TIMEOUT, collect).await {
        Ok(r) => r,
        Err(_) => {
            tracing::warn!(
                "chat agent priors timed out after {}s",
                PRIOR_BATCH_TIMEOUT.as_secs()
            );
            Vec::new()
        }
    };
    ctx.push_str(&format_priors_prompt_block(&results, true, attempted));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_block_mentions_sidecar() {
        let s = format_priors_prompt_block(&[], false, 2);
        assert!(s.contains("offline"));
        assert!(s.contains("SIDECAR MODEL PRIORS"));
    }

    #[test]
    fn empty_online_block() {
        let s = format_priors_prompt_block(&[], true, 3);
        assert!(s.contains("No agent opinions"));
    }
}
