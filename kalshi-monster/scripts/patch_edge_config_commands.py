#!/usr/bin/env python3
"""One-shot edits to commands/mod.rs for edge config + paper breaker helper."""
from pathlib import Path

path = Path("kalshi-monster/src-tauri/src/commands/mod.rs")
text = path.read_text(encoding="utf-8")

helper = """
async fn edge_config_for_pool(db_pool: &Pool<Sqlite>) -> crate::edge_engine::EdgeConfig {
    match crate::edge_engine::persistence::load_edge_config(db_pool).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("edge config load failed, using defaults: {e}");
            crate::edge_engine::EdgeConfig::default()
        }
    }
}

"""

anchor = "async fn analyze_market_edge_inner("
if "async fn edge_config_for_pool" not in text:
    if anchor not in text:
        raise SystemExit("anchor not found")
    text = text.replace(anchor, helper + anchor, 1)

text = text.replace(
    "        &crate::edge_engine::EdgeConfig::default(),\n    )\n    .await\n}\n\n/// Run sidecar agents",
    "        &edge_config_for_pool(db_pool).await,\n    )\n    .await\n}\n\n/// Run sidecar agents",
    1,
)

old_refit = """/// Re-fit shrinkage lambda from resolved forecast ledger (plan §4.1).
/// Returns None when fewer than LAMBDA_REFIT_MIN_SAMPLES rows carry a model opinion.
#[tauri::command]
pub async fn kalshi_refit_lambda(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Option<crate::edge_engine::calibration::LambdaFit>, String> {
    let resolved = crate::kalshi::forecast::resolved_forecasts_for_calibration(&db_pool).await?;
    Ok(crate::edge_engine::calibration::refit_lambda(
        &resolved,
        crate::edge_engine::calibration::LAMBDA_REFIT_MIN_SAMPLES,
    ))
}"""

new_refit = """/// Re-fit shrinkage lambda from resolved forecast ledger (plan §4.1).
/// Returns None when fewer than LAMBDA_REFIT_MIN_SAMPLES rows carry a model opinion.
/// On success, persists λ to SQLite edge config for subsequent evaluate/analyze paths.
#[tauri::command]
pub async fn kalshi_refit_lambda(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Option<crate::edge_engine::calibration::LambdaFit>, String> {
    let resolved = crate::kalshi::forecast::resolved_forecasts_for_calibration(&db_pool).await?;
    let fit = crate::edge_engine::calibration::refit_lambda(
        &resolved,
        crate::edge_engine::calibration::LAMBDA_REFIT_MIN_SAMPLES,
    );
    if let Some(ref f) = fit {
        crate::edge_engine::persistence::save_shrinkage_lambda(&db_pool, f.lambda).await?;
    }
    Ok(fit)
}

/// Load persisted edge tunables (shrinkage λ and defaults for other fields).
#[tauri::command]
pub async fn kalshi_get_edge_config(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::edge_engine::EdgeConfig, String> {
    crate::edge_engine::persistence::load_edge_config(&db_pool).await
}"""

if old_refit not in text:
    raise SystemExit("refit block not found")
text = text.replace(old_refit, new_refit, 1)

# Paper path evaluate uses persisted edge config
text = text.replace(
    "        let edge = crate::edge_engine::evaluate(\n            &opinion,\n            quote,\n            &flags,\n            &crate::edge_engine::EdgeConfig::default(),\n        );",
    "        let edge_cfg = edge_config_for_pool(&db_pool).await;\n        let edge = crate::edge_engine::evaluate(\n            &opinion,\n            quote,\n            &flags,\n            &edge_cfg,\n        );",
    1,
)

old_paper_breaker = """    let breaker_mult_applied = (breaker_stake_mult - 1.0).abs() > 1e-9;

    let raw_stake = if decision.recommended_stake_dollars > 0.0 {
        decision.recommended_stake_dollars
    } else {
        bankroll.total_bankroll * (decision.fractional_kelly_pct / 100.0)
    };
    let raw_stake = raw_stake * breaker_stake_mult;

    if breaker_mult_applied {
        if !decision
            .risk_flags
            .contains(&crate::chat::decision_schema::RiskFlag::CircuitBreakerActive)
        {
            decision
                .risk_flags
                .push(crate::chat::decision_schema::RiskFlag::CircuitBreakerActive);
        }
        let note = format!(
            "breaker stake_multiplier {:.2} applied (paper path)",
            breaker_stake_mult
        );
        if !decision.thesis.is_empty() {
            decision.thesis.push(' ');
        }
        decision.thesis.push_str(&note);
    }"""

new_paper_breaker = """    let base_stake = if decision.recommended_stake_dollars > 0.0 {
        decision.recommended_stake_dollars
    } else {
        bankroll.total_bankroll * (decision.fractional_kelly_pct / 100.0)
    };
    let breaker_apply = crate::edge_engine::paper_breaker::apply_paper_breaker_stake(
        base_stake,
        breaker_stake_mult,
    );
    let raw_stake = breaker_apply.adjusted_stake;

    if breaker_apply.multiplier_applied {
        if !decision
            .risk_flags
            .contains(&crate::chat::decision_schema::RiskFlag::CircuitBreakerActive)
        {
            decision
                .risk_flags
                .push(crate::chat::decision_schema::RiskFlag::CircuitBreakerActive);
        }
        if let Some(note) = breaker_apply.thesis_note {
            if !decision.thesis.is_empty() {
                decision.thesis.push(' ');
            }
            decision.thesis.push_str(&note);
        }
    }"""

if old_paper_breaker not in text:
    raise SystemExit("paper breaker block not found")
text = text.replace(old_paper_breaker, new_paper_breaker, 1)

path.write_text(text, encoding="utf-8")
print("commands/mod.rs updated OK")