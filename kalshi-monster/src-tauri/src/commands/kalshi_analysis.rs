#![allow(unused_imports)]

use crate::commands::{KalshiState, SharedCacheState, PickInput, KalshiDashboardBootstrap, edge_config_for_pool, emit_chat_kalshi_context};
use crate::chat::openrouter::{self, OpenRouterResponse};
use crate::chat::session;
use crate::config;
use crate::config::AppConfig;
use crate::error::AppError;
use crate::secrets::{AppSecrets, SecretKey};
use crate::football::data;
use crate::football::live_data;
use crate::football::player_stats;
use crate::predictions::tracker::{PredictionOutcome, PredictionRecord, PredictionTracker};
use crate::predictions::grading::{self, GradingSummary};
use crate::weather::WeatherClient;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tauri::{Emitter, State};
use tokio::sync::{Mutex, mpsc};

// ═══════════════════════════════════════════════════════════════
// Kalshi Prediction Tracking — Performance & Grading
// ═══════════════════════════════════════════════════════════════

use crate::kalshi::models::{KalshiPrediction, KalshiPredictionStats, KalshiGradingSummary};

/// Get all Kalshi predictions
#[tauri::command]
pub async fn kalshi_get_predictions(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<Vec<KalshiPrediction>, String> {
    let t = tracker.lock().await;
    Ok(t.get_kalshi_predictions().await)
}

/// Get Kalshi prediction stats
#[tauri::command]
pub async fn kalshi_get_prediction_stats(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
) -> Result<KalshiPredictionStats, String> {
    let t = tracker.lock().await;
    let all = t.get_kalshi_predictions().await;
    Ok(t.get_kalshi_stats(&all).await)
}

/// Grade pending Kalshi predictions against resolved market outcomes.
/// Also settles open paper lots and syncs prediction outcomes from those lots.
#[tauri::command]
pub async fn kalshi_grade_pending_predictions(
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<KalshiGradingSummary, String> {
    let summary = {
        let t = tracker.lock().await;
        crate::kalshi::grade_pending_predictions(&t, &kalshi, &db_pool).await?
    };
    // Paper lots + prediction sync (side-aware settlement).
    match crate::paper::settle_pending(&db_pool, &kalshi).await {
        Ok(ps) if ps.settled > 0 => {
            tracing::info!(
                "grade path also settled {} paper lot(s) ({}W/{}L ${:.2})",
                ps.settled,
                ps.wins,
                ps.losses,
                ps.total_pnl
            );
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("paper settle during grade: {e}"),
    }
    // Forecast ledger for any remaining open markets.
    if let Err(e) = crate::kalshi::grading::resolve_pending_forecasts(&db_pool, &kalshi).await {
        tracing::warn!("forecast resolve during grade: {e}");
    }
    Ok(summary)
}

/// Portfolio-aware Kelly stake scaling for a proposed Kalshi trade.
#[tauri::command]
pub async fn kalshi_compute_stake_adjustment(
    ticker: String,
    category: String,
    contract_side: String,
    recommended_stake: f64,
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::kalshi::StakeAdjustment, String> {
    let pending = {
        let t = tracker.lock().await;
        t.get_kalshi_predictions().await
    };
    let mut exposures = crate::kalshi::exposures_from_predictions(
        &pending
            .iter()
            .filter(|p| p.actual_outcome.is_none())
            .cloned()
            .collect::<Vec<_>>(),
    );

    if let Ok(positions) = kalshi.get_positions().await {
        exposures.extend(crate::kalshi::exposures_from_positions(&positions));
    }

    let mut adjustment = crate::kalshi::compute_stake_adjustment(
        &ticker,
        &category,
        Some(&contract_side),
        recommended_stake,
        &exposures,
    );

    let bankroll = crate::bankroll::load_bankroll_config();
    match crate::bankroll::get_bankroll_summary_synced(&bankroll, &db_pool).await {
        Ok(summary) => {
            adjustment.remaining_daily = summary.remaining_daily;
            adjustment.remaining_weekly = summary.remaining_weekly;
            adjustment.bankroll_cap = summary.remaining_daily.min(summary.remaining_weekly);
            let (capped_stake, warning) = crate::bankroll::apply_bankroll_cap(
                adjustment.adjusted_recommended_stake,
                &summary,
            );
            if capped_stake < adjustment.adjusted_recommended_stake {
                let old = adjustment.adjusted_recommended_stake;
                adjustment.adjusted_recommended_stake = capped_stake;
                adjustment.kelly_scale = if old > 0.0 {
                    (capped_stake / old).clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
            if let Some(warning) = warning {
                adjustment.warnings.push(warning);
            }
        }
        Err(e) => {
            tracing::warn!("bankroll cap sync skipped for stake adjustment: {}", e);
        }
    }

    Ok(adjustment)
}

#[tauri::command]
pub async fn kalshi_get_calibration_status(
    raw_probability_pct: f64,
) -> Result<crate::analysis::calibration::CalibrationStatus, String> {
    Ok(crate::analysis::calibration::calibration_status_for_probability(
        raw_probability_pct,
    ))
}


async fn analyze_market_edge_inner(
    ticker: &str,
    client: &crate::kalshi::KalshiClient,
    bridge: &crate::fincept_bridge::FinceptBridge,
    db_pool: &Pool<Sqlite>,
) -> Result<crate::edge_engine::pipeline::EdgeAnalysisResult, String> {
    // Shared builder: mids from price_tracker + underlying/strike inference (Phase A).
    let input =
        crate::edge_engine::opinion_input::build_analyze_input(client, db_pool, ticker).await?;

    crate::edge_engine::pipeline::analyze_and_log_forecast(
        db_pool,
        bridge,
        input,
        &edge_config_for_pool(db_pool).await,
    )
    .await
}

/// Run sidecar agents + edge_engine on one market; write a forecast ledger row
/// (including PASS). Primary path for real p_model accumulation.
#[tauri::command]
pub async fn kalshi_analyze_market_edge(
    ticker: String,
    kalshi: State<'_, KalshiState>,
    bridge: State<'_, Arc<crate::fincept_bridge::FinceptBridge>>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::edge_engine::pipeline::EdgeAnalysisResult, String> {
    let client = kalshi.as_ref();
    analyze_market_edge_inner(&ticker, &client, bridge.as_ref(), &db_pool).await
}

/// Analyze the top-N open markets by volume (from tape) and log forecasts.
/// Does **not** invent outcomes — only writes pending rows for live markets.
#[tauri::command]
pub async fn kalshi_analyze_top_markets_edge(
    limit: Option<usize>,
    kalshi: State<'_, KalshiState>,
    bridge: State<'_, Arc<crate::fincept_bridge::FinceptBridge>>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<Vec<crate::edge_engine::pipeline::EdgeAnalysisResult>, String> {
    let n = limit.unwrap_or(10).min(25);
    let client = kalshi.as_ref();
    let top = client.get_top_markets(n).await?;
    let mut out = Vec::new();
    for summary in top {
        match analyze_market_edge_inner(
            &summary.ticker,
            &client,
            bridge.as_ref(),
            &db_pool,
        )
        .await
        {
            Ok(r) => out.push(r),
            Err(e) => {
                tracing::warn!("edge analyze {}: {e}", summary.ticker);
            }
        }
    }
    Ok(out)
}

/// Poll Kalshi for settlement of unresolved forecast rows; compute Brier scores.
#[tauri::command]
pub async fn kalshi_resolve_pending_forecasts(
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<u32, String> {
    let client = kalshi.as_ref();
    let n = crate::kalshi::grading::resolve_pending_forecasts(&db_pool, &client).await?;
    // Keep paper journal in lockstep with forecast resolution.
    if let Err(e) = crate::paper::settle_pending(&db_pool, client).await {
        tracing::warn!("paper settle during forecast resolve: {e}");
    }
    Ok(n)
}

/// Calibration gate report from the **forecast** ledger (real rows only).
#[derive(Debug, serde::Serialize)]
pub struct ForecastCalibrationReport {
    pub resolved_count: i64,
    pub unresolved_count: i64,
    pub brier_market: Option<f64>,
    pub brier_final: Option<f64>,
    pub brier_model: Option<f64>,
    pub brier_market_on_model_rows: Option<f64>,
    pub n_model: usize,
    pub gate_passed: bool,
    pub gate_reasons: Vec<String>,
    pub paper_pnl: Option<f64>,
    /// 10-bucket reliability diagram for p_final (empty when no resolved rows).
    pub reliability_final: Vec<crate::edge_engine::calibration::ReliabilityBucket>,
    /// Same buckets using p_market for comparison.
    pub reliability_market: Vec<crate::edge_engine::calibration::ReliabilityBucket>,
}

#[tauri::command]
pub async fn kalshi_get_forecast_calibration_report(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<ForecastCalibrationReport, String> {
    let resolved = crate::kalshi::forecast::resolved_forecasts_for_calibration(&db_pool).await?;
    let resolved_count = crate::kalshi::forecast::resolved_count(&db_pool).await?;
    let unresolved_count = crate::kalshi::forecast::unresolved_forecasts(&db_pool)
        .await?
        .len() as i64;

    let summary = crate::edge_engine::calibration::brier_summary(&resolved);
    let paper_pnl = crate::paper::get_analytics(&db_pool, None)
        .await
        .ok()
        .map(|a| a.realized_pnl);

    let gate = crate::edge_engine::calibration::evaluate_gate(
        &resolved,
        paper_pnl.unwrap_or(0.0),
        &crate::edge_engine::calibration::GateConfig::default(),
    );

    let pairs_final: Vec<(f64, bool)> = resolved
        .iter()
        .map(|r| (r.p_final, r.outcome))
        .collect();
    let pairs_market: Vec<(f64, bool)> = resolved
        .iter()
        .map(|r| (r.p_market, r.outcome))
        .collect();
    let reliability_final =
        crate::edge_engine::calibration::reliability_diagram(&pairs_final, 10);
    let reliability_market =
        crate::edge_engine::calibration::reliability_diagram(&pairs_market, 10);

    Ok(ForecastCalibrationReport {
        resolved_count,
        unresolved_count,
        brier_market: summary.as_ref().map(|s| s.brier_market),
        brier_final: summary.as_ref().map(|s| s.brier_final),
        brier_model: summary.as_ref().and_then(|s| s.brier_model),
        brier_market_on_model_rows: summary
            .as_ref()
            .and_then(|s| s.brier_market_on_model_rows),
        n_model: summary.as_ref().map(|s| s.n_model).unwrap_or(0),
        gate_passed: gate.passed,
        gate_reasons: gate.conditions,
        paper_pnl,
        reliability_final,
        reliability_market,
    })
}

/// Snapshot current Kalshi market prices into local history.
#[tauri::command]
pub async fn kalshi_snapshot_prices(
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::kalshi::KalshiSnapshotBatch, String> {
    let client = kalshi.as_ref();
    let markets = client.fetch_all_markets().await?;
    let summaries: Vec<crate::kalshi::KalshiMarketSummary> =
        markets.iter().map(crate::kalshi::KalshiMarketSummary::from).collect();
    crate::kalshi::price_tracker::snapshot_markets(&db_pool, &summaries).await
}

/// Fetch stored price/spread history for a ticker.
#[tauri::command]
pub async fn kalshi_get_price_history(
    ticker: String,
    limit: Option<i64>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::kalshi::KalshiPriceHistory, String> {
    crate::kalshi::price_tracker::get_price_history(&db_pool, &ticker, limit.unwrap_or(200)).await
}

/// Re-fit shrinkage lambda from resolved forecast ledger (plan §4.1).
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
        // NaN for non-lambda fields keeps previous values
        crate::edge_engine::persistence::save_edge_config(&db_pool, f.lambda, f64::NAN, f64::NAN, f64::NAN, f64::NAN).await?;
    }
    Ok(fit)
}

/// Load persisted edge tunables (shrinkage λ and defaults for other fields).
#[tauri::command]
pub async fn kalshi_get_edge_config(
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::edge_engine::EdgeConfig, String> {
    crate::edge_engine::persistence::load_edge_config(&db_pool).await
}

/// Persist edge-engine tunables (plan §4.1, Appendix C).  Any field passed as 0.0 or
/// non-finite keeps its previous value.  Returns the loaded config after persistence.
#[tauri::command]
pub async fn kalshi_set_edge_config(
    shrinkage_lambda: f64,
    min_edge: f64,
    fee_multiplier: f64,
    kelly_fraction: f64,
    min_confidence: f64,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<crate::edge_engine::EdgeConfig, String> {
    crate::edge_engine::persistence::save_edge_config(
        &db_pool,
        shrinkage_lambda,
        min_edge,
        fee_multiplier,
        kelly_fraction,
        min_confidence,
    )
    .await
}

/// Record a paper-trade decision with calibration + correlation-adjusted sizing.
#[tauri::command]
pub async fn kalshi_record_paper_decision(
    session_id: String,
    mut decision: crate::chat::decision_schema::KalshiTradeDecision,
    tracker: State<'_, Arc<Mutex<crate::predictions::tracker::PredictionTracker>>>,
    kalshi: State<'_, KalshiState>,
    db_pool: State<'_, Pool<Sqlite>>,
) -> Result<String, String> {
    let bankroll = crate::bankroll::load_bankroll_config();
    // Normalize mixed LLM units (0–1 dollars vs percent/cents) and hard-cap Kelly/stake
    // before TAKE is persisted or shown as actionable paper size.
    decision.sanitize_units_and_caps(
        bankroll.total_bankroll,
        bankroll.kelly_fraction,
        bankroll.max_bet_pct,
    );
    // Quality rails (no math changes): placeholder tickers, spread>edge, longshot multiplies.
    decision.enforce_prediction_quality_rails();
    if crate::chat::decision_schema::KalshiTradeDecision::is_placeholder_ticker(&decision.ticker) {
        return Err(
            "Invalid/placeholder ticker — refuse to record paper decision (use a real KX… ticker from tape)"
                .into(),
        );
    }

    // Hard settlement rail: tape SETTLED/CLOSED (incl. embedded ticker dates) → force PASS.
    // Re-applied after Kelly recompute so stake cannot reappear.
    let settlement_gate = {
        let client = kalshi.as_ref();
        let mkt = if let Some(m) = client.find_cached_market(&decision.ticker) {
            Some(m)
        } else {
            client.fetch_market(&decision.ticker).await.ok()
        };
        if let Some(m) = mkt {
            crate::chat::market_gate::assess_market_gate_for_ticker(
                Some(&m.ticker),
                &m.status,
                &m.result,
                m.close_time.as_deref(),
                m.expiration_time.as_deref(),
                chrono::Utc::now(),
            )
        } else {
            crate::chat::market_gate::assess_market_gate_for_ticker(
                Some(&decision.ticker),
                "unknown",
                "",
                None,
                None,
                chrono::Utc::now(),
            )
        }
    };
    if !settlement_gate.allows_take() {
        tracing::info!(
            "paper decision forced PASS by settlement gate: {} {:?}",
            decision.ticker,
            settlement_gate
        );
    }
    decision.enforce_settlement_gate(&settlement_gate);

    let bankroll_summary = match crate::bankroll::get_bankroll_summary_synced(&bankroll, &db_pool).await {
        Ok(summary) => Some(summary),
        Err(e) => {
            tracing::warn!("bankroll cap sync skipped for paper decision: {}", e);
            None
        }
    };
    let pending = {
        let t = tracker.lock().await;
        t.get_kalshi_predictions().await
    };
    let mut exposures = crate::kalshi::exposures_from_predictions(
        &pending
            .iter()
            .filter(|p| p.actual_outcome.is_none())
            .cloned()
            .collect::<Vec<_>>(),
    );
    if let Ok(positions) = kalshi.get_positions().await {
        exposures.extend(crate::kalshi::exposures_from_positions(&positions));
    }

    let side = format!("{:?}", decision.contract_side);

    // Circuit-breaker stake multiplier (§6.4): scale paper stakes when drawdown or
    // calibration degradation is active.
    let breaker_stake_mult =
        match crate::edge_engine::persistence::evaluate_and_persist_breakers(&db_pool).await {
            Ok(bd) => bd.stake_multiplier,
            Err(e) => {
                tracing::warn!("breaker evaluation skipped for paper decision: {e}");
                1.0
            }
        };
    let base_stake = if decision.recommended_stake_dollars > 0.0 {
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
    }

    let mut adj = crate::kalshi::compute_stake_adjustment(
        &decision.ticker,
        &decision.category,
        Some(&side),
        raw_stake,
        &exposures,
    );
    decision.compute_risk_adjusted_with_policy(
        bankroll.total_bankroll,
        bankroll.kelly_fraction,
        adj.kelly_scale,
        true,
        bankroll.max_bet_pct,
    );
    // Kelly recompute must not resurrect a TAKE on a settled market.
    decision.enforce_settlement_gate(&settlement_gate);
    // Re-apply quality rails after Kelly so stakes stay zero on demoted decisions.
    decision.enforce_prediction_quality_rails();

    if let Some(summary) = &bankroll_summary {
        let (capped_stake, warning) =
            crate::bankroll::apply_bankroll_cap(decision.recommended_stake_dollars, summary);
        if capped_stake < decision.recommended_stake_dollars {
            let old_stake = decision.recommended_stake_dollars;
            decision.recommended_stake_dollars = capped_stake;
            decision.max_position_dollars = decision.max_position_dollars.min(capped_stake);
            if !decision.risk_flags.contains(&crate::chat::decision_schema::RiskFlag::BankrollLimitExceeded) {
                decision.risk_flags.push(crate::chat::decision_schema::RiskFlag::BankrollLimitExceeded);
            }
            if let Some(warning) = warning {
                adj.warnings.push(warning.clone());
                if !decision.thesis.is_empty() {
                    decision.thesis.push(' ');
                }
                decision.thesis.push_str(&warning);
            }
            tracing::info!(
                "paper decision capped by bankroll: {} ${:.2} -> ${:.2}",
                decision.ticker,
                old_stake,
                capped_stake
            );
        }
    }

    let prediction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let decision_json = serde_json::to_string(&decision)
        .map_err(|e| format!("serialize decision: {}", e))?;
    let pick_type = match decision.contract_side {
        crate::chat::decision_schema::ContractSide::YES => Some("Over".to_string()),
        crate::chat::decision_schema::ContractSide::NO => Some("Under".to_string()),
        crate::chat::decision_schema::ContractSide::PASS => None,
    };

    let prediction = crate::predictions::tracker::Prediction {
        id: prediction_id.clone(),
        session_id,
        raw_text: decision_json.clone(),
        player_name: Some(decision.ticker.clone()),
        pick_type,
        line: Some(decision.recommended_stake_dollars),
        stat_category: Some(decision.category.clone()),
        confidence: Some(format!("{:?}", decision.confidence_tier)),
        confidence_score: None,
        probability: Some(decision.fair_probability_pct),
        reasoning: if decision.thesis.is_empty() {
            None
        } else {
            Some(decision.thesis.clone())
        },
        risk: if adj.warnings.is_empty() {
            None
        } else {
            Some(adj.warnings.join("; "))
        },
        created_at: now.clone(),
                full_decision_json: Some(decision_json.clone()),
        // Store selected-side entry in dollars [0,1] (not market_price_pct which is 0–100).
        entry_price: Some(decision.price_to_enter),
        model_disagreement: decision.model_disagreement,
    };

    let record = PredictionRecord {
        prediction,
        outcome: PredictionOutcome::Pending,
        actual_result: None,
        notes: Some(format!(
            "Paper trade: {:?} {} @ {:.2} (kelly_scale {:.0}%)",
            decision.contract_side,
            decision.ticker,
            decision.price_to_enter,
            adj.kelly_scale * 100.0
        )),
        resolved_at: None,
    };

    // Forecast ledger via edge_engine: shrink LLM fair-prob toward market mid,
    // apply fee-aware verdict. Agent pipeline (sidecar) is the preferred source
    // of p_model; paper path uses the decision's fair probability as a single
    // model opinion so columns are never left blank with a raw unshrunk number.
    let close_time = chrono::Utc::now().to_rfc3339();
    // market_price_pct is percent 0–100; price_to_enter is dollars 0–1 after sanitize.
    let p_market_raw = (decision.market_price_pct / 100.0).clamp(0.01, 0.99);
    let p_model_raw = (decision.fair_probability_pct / 100.0).clamp(0.01, 0.99);
    let yes_ask = crate::chat::decision_schema::coerce_price_to_dollars(decision.price_to_enter)
        .clamp(0.01, 0.99);
    // Approximate bid as mid-symmetric when only entry price is known.
    let yes_bid = (2.0 * p_market_raw - yes_ask).clamp(0.0, 1.0);
    let quote = crate::edge_engine::Quote { yes_bid, yes_ask };
    let opinion = crate::edge_engine::ModelOpinion {
        p_model: p_model_raw,
        confidence: match decision.confidence_tier {
            crate::chat::decision_schema::ConfidenceTier::High => 0.75,
            crate::chat::decision_schema::ConfidenceTier::Medium => 0.50,
            crate::chat::decision_schema::ConfidenceTier::Low => 0.30,
            crate::chat::decision_schema::ConfidenceTier::None => 0.0,
        },
        contributions: vec![crate::edge_engine::AgentContribution {
            agent: "llm_decision".into(),
            probability: p_model_raw,
            confidence: 0.5,
            weight_normalized: 1.0,
        }],
    };
    let mut flags: Vec<String> = adj.warnings.clone();
    if decision.contract_side == crate::chat::decision_schema::ContractSide::PASS {
        flags.push("user_or_model_pass".into());
    }
    let edge_cfg = edge_config_for_pool(&db_pool).await;
    let edge = crate::edge_engine::evaluate(&opinion, quote, &flags, &edge_cfg);
    let verdict = match edge.verdict {
        crate::edge_engine::Verdict::TradeYes => "trade_yes",
        crate::edge_engine::Verdict::TradeNo => "trade_no",
        crate::edge_engine::Verdict::Pass => "pass",
    };
    let verdict_reasons =
        serde_json::to_string(&edge.reasons).unwrap_or_else(|_| "[]".to_string());
    let stake_suggested = if decision.recommended_stake_dollars > 0.0 {
        Some(decision.recommended_stake_dollars)
    } else {
        None
    };
    let breakdown = serde_json::to_string(&opinion.contributions).ok();

    // Open a paper lot only on actionable TAKE with a real side + stake.
    // WATCH/PASS must not debit cash even if contract_side is YES/NO.
    let trade_input = if decision.decision == crate::chat::decision_schema::DecisionAction::TAKE
        && decision.contract_side != crate::chat::decision_schema::ContractSide::PASS
    {
        let entry_cents = crate::paper::normalize_entry_cents(decision.price_to_enter);
        let stake = decision.recommended_stake_dollars.max(0.0);
        if stake > 0.0 && entry_cents > 0.0 && entry_cents < 100.0 {
            let qty = stake / (entry_cents / 100.0);
            let side = format!("{:?}", decision.contract_side);
            Some(crate::paper::PaperTradeInput {
                ticker: decision.ticker.clone(),
                title: decision.market_title.clone(),
                category: decision.category.clone(),
                side,
                qty,
                entry_price_cents: entry_cents,
                source: crate::paper::PaperTradeSource::AiDecision,
                decision_json: Some(decision_json.clone()),
                prediction_id: Some(prediction_id.clone()),
            })
        } else {
            None
        }
    } else {
        None
    };

    let ctx = crate::paper::PaperDecisionContext {
        prediction: record,
        forecast_ticker: decision.ticker.clone(),
        forecast_created_at: now.clone(),
        forecast_close_time: close_time,
        p_market: edge.p_market,
        p_model: Some(edge.p_model),
        p_final: edge.p_final,
        verdict: verdict.to_string(),
        verdict_reasons,
        stake_suggested,
        agent_breakdown: breakdown,
        trade_input: trade_input.clone(),
    };

    let prediction_id = crate::paper::record_paper_decision(&db_pool, ctx).await?;

    if trade_input.is_some() {
        tracing::info!(
            "paper decision recorded: {} {:?} (prediction {})",
            decision.ticker,
            decision.contract_side,
            prediction_id
        );
    }

    Ok(prediction_id)
}

// ═══════════════════════════════════════════════════════════════
