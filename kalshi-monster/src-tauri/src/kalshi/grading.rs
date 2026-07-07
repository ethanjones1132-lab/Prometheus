//! Contract-side-aware Kalshi grading and binary PnL.

use super::forecast;
use super::models::KalshiPrediction;
use crate::kalshi::client::KalshiClient;
use crate::kalshi::models::{KalshiGradingResult, KalshiGradingSummary};
use crate::notification::{self, AppNotification, NotificationType};
use crate::predictions::tracker::PredictionTracker;
use sqlx::Pool;
use sqlx::Sqlite;
use std::collections::HashMap;
use tauri::Emitter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KalshiBetSide {
    Yes,
    No,
    Pass,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct KalshiBetEvaluation {
    pub side: KalshiBetSide,
    pub won: bool,
    pub pnl: f64,
    pub entry_price: f64,
    pub market_price_at_entry_pct: Option<f64>,
}

pub fn parse_bet_side(contract_side: Option<&str>, pick_type: Option<&str>) -> KalshiBetSide {
    if let Some(raw) = contract_side {
        let upper = raw.trim().to_uppercase();
        if upper == "YES" {
            return KalshiBetSide::Yes;
        }
        if upper == "NO" {
            return KalshiBetSide::No;
        }
        if upper == "PASS" {
            return KalshiBetSide::Pass;
        }
    }
    if let Some(pt) = pick_type {
        let lower = pt.trim().to_lowercase();
        if lower == "over" {
            return KalshiBetSide::Yes;
        }
        if lower == "under" {
            return KalshiBetSide::No;
        }
    }
    KalshiBetSide::Unknown
}

pub fn infer_market_price_at_entry(
    stored_market_price: Option<f64>,
    price_to_enter: Option<f64>,
    contract_side: Option<&str>,
) -> Option<f64> {
    if let Some(m) = stored_market_price {
        return Some(m);
    }
    let entry = price_to_enter?;
    let entry_dec = if entry > 0.0 && entry < 1.0 {
        entry
    } else if entry > 1.0 && entry <= 100.0 {
        entry / 100.0
    } else {
        return None;
    };
    match parse_bet_side(contract_side, None) {
        KalshiBetSide::Yes => Some(entry_dec * 100.0),
        KalshiBetSide::No => Some((1.0 - entry_dec) * 100.0),
        _ => None,
    }
}

pub fn market_price_at_entry_pct(pred: &KalshiPrediction) -> Option<f64> {
    infer_market_price_at_entry(
        pred.market_price_at_entry,
        pred.price_to_enter,
        pred.contract_side.as_deref(),
    )
}

fn entry_price_decimal(pred: &KalshiPrediction, side: KalshiBetSide) -> f64 {
    if let Some(p) = pred.price_to_enter {
        if p > 0.0 && p < 1.0 {
            return p;
        }
        if p > 1.0 && p <= 100.0 {
            return p / 100.0;
        }
    }
    if let Some(m) = pred.market_price_at_entry {
        let yes = m / 100.0;
        return match side {
            KalshiBetSide::Yes => yes,
            KalshiBetSide::No => 1.0 - yes,
            _ => 0.5,
        };
    }
    0.5
}

pub fn bet_won(side: KalshiBetSide, actual_outcome: &str) -> Option<bool> {
    match side {
        KalshiBetSide::Yes => Some(actual_outcome == "Yes"),
        KalshiBetSide::No => Some(actual_outcome == "No"),
        KalshiBetSide::Pass | KalshiBetSide::Unknown => None,
    }
}

pub fn contract_pnl(stake: f64, entry_price: f64, won: bool) -> f64 {
    if stake <= 0.0 {
        return 0.0;
    }
    if !won {
        return -stake;
    }
    let p = entry_price.clamp(0.01, 0.99);
    (stake / p) - stake
}

pub fn evaluate_bet(pred: &KalshiPrediction, actual_outcome: &str) -> Option<KalshiBetEvaluation> {
    let side = parse_bet_side(
        pred.contract_side.as_deref(),
        pred.pick_type.as_deref(),
    );
    let won = bet_won(side, actual_outcome)?;
    let entry_price = entry_price_decimal(pred, side);
    let pnl = contract_pnl(pred.stake_amount, entry_price, won);
    Some(KalshiBetEvaluation {
        side,
        won,
        pnl,
        entry_price,
        market_price_at_entry_pct: market_price_at_entry_pct(pred),
    })
}

pub async fn grade_pending_predictions(
    tracker: &PredictionTracker,
    client: &KalshiClient,
    pool: &Pool<Sqlite>,
) -> Result<KalshiGradingSummary, String> {
    let pending: Vec<KalshiPrediction> = tracker
        .get_kalshi_predictions()
        .await
        .into_iter()
        .filter(|p| p.actual_outcome.is_none())
        .collect();

    if pending.is_empty() {
        return Ok(empty_summary());
    }

    let mut by_ticker: HashMap<String, Vec<&KalshiPrediction>> = HashMap::new();
    for pred in &pending {
        by_ticker.entry(pred.ticker.clone()).or_default().push(pred);
    }

    let mut results = Vec::new();
    let mut wins = 0u32;
    let mut losses = 0u32;
    let mut total_pnl = 0.0;

    for (ticker, preds) in by_ticker {
        let market = match client.fetch_market(&ticker).await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("kalshi grade: skip {} — {}", ticker, e);
                continue;
            }
        };
        if market.result.is_empty() {
            continue;
        }

        let actual = market.result.clone();
        let resolved_at = chrono::Utc::now().to_rfc3339();

        if let Err(e) =
            forecast::resolve_forecasts_for_market(pool, &ticker, &actual, &resolved_at).await
        {
            tracing::warn!("kalshi forecast resolve for {ticker}: {e}");
        }

        for pred in preds {
            let Some(eval) = evaluate_bet(pred, &actual) else {
                continue;
            };
            if eval.won {
                wins += 1;
            } else {
                losses += 1;
            }
            total_pnl += eval.pnl;
            tracker
                .update_kalshi_outcome(&pred.id, &actual, eval.pnl)
                .await?;

            // CLV tracking: record the close price (last market price before resolution)
            // For binary markets: Yes win = close at 1.0, No win = close at 0.0
            let close_price = if actual == "Yes" { 1.0 } else { 0.0 };
            if let Err(e) = tracker.update_prediction_clv(&pred.id, close_price).await {
                tracing::warn!("kalshi CLV update failed for {}: {}", pred.id, e);
            }

            results.push(KalshiGradingResult {
                prediction_id: pred.id.clone(),
                ticker: pred.ticker.clone(),
                title: pred.title.clone(),
                category: pred.category.clone(),
                predicted_probability: pred.predicted_probability,
                actual_outcome: actual.clone(),
                outcome: if eval.won {
                    "Win".to_string()
                } else {
                    "Loss".to_string()
                },
                pnl: eval.pnl,
                stake_amount: pred.stake_amount,
                contract_side: Some(side_label(eval.side)),
                market_price_at_entry: eval.market_price_at_entry_pct,
                notes: None,
                resolved_at: resolved_at.clone(),
            });
        }
    }

    Ok(KalshiGradingSummary {
        total_predictions: pending.len() as u32,
        pending_gradable: pending.len() as u32,
        graded: results.len() as u32,
        wins,
        losses,
        total_pnl,
        results,
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

fn empty_summary() -> KalshiGradingSummary {
    KalshiGradingSummary {
        total_predictions: 0,
        pending_gradable: 0,
        graded: 0,
        wins: 0,
        losses: 0,
        total_pnl: 0.0,
        results: vec![],
        fetched_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// Resolve forecast ledger rows whose markets have settled on Kalshi (no prediction rows required).
pub async fn resolve_pending_forecasts(
    pool: &Pool<Sqlite>,
    client: &KalshiClient,
) -> Result<u32, String> {
    use std::collections::HashSet;

    let unresolved = forecast::unresolved_forecasts(pool).await?;
    if unresolved.is_empty() {
        return Ok(0);
    }

    let tickers: HashSet<String> = unresolved
        .iter()
        .map(|f| f.market_ticker.clone())
        .collect();
    let resolved_at = chrono::Utc::now().to_rfc3339();
    let mut total = 0u32;

    for ticker in tickers {
        let market = match client.fetch_market(&ticker).await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("forecast poller: skip {ticker} — {e}");
                continue;
            }
        };
        if market.result.is_empty() {
            continue;
        }
        total += forecast::resolve_forecasts_for_market(
            pool,
            &ticker,
            &market.result,
            &resolved_at,
        )
        .await?;
    }

    Ok(total)
}

pub fn spawn_auto_grade_task(
    kalshi: std::sync::Arc<tokio::sync::Mutex<KalshiClient>>,
    tracker: std::sync::Arc<tokio::sync::Mutex<PredictionTracker>>,
    pool: Pool<Sqlite>,
    app_handle: tauri::AppHandle,
    poll_interval_secs: u64,
) {
    let interval_secs = poll_interval_secs.max(60);
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let pending_count = {
                let t = tracker.lock().await;
                t.get_kalshi_predictions()
                    .await
                    .into_iter()
                    .filter(|p| p.actual_outcome.is_none())
                    .count()
            };
            let forecast_pending = forecast::unresolved_forecasts(&pool)
                .await
                .map(|v| v.len())
                .unwrap_or(0);
            if pending_count == 0 && forecast_pending == 0 {
                continue;
            }
            let summary = {
                let t = tracker.lock().await;
                let client = kalshi.lock().await;
                if pending_count > 0 {
                    match grade_pending_predictions(&t, &client, &pool).await {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("kalshi auto-grade: {e}");
                            continue;
                        }
                    }
                } else {
                    empty_summary()
                }
            };
            if forecast_pending > 0 {
                let client = kalshi.lock().await;
                match resolve_pending_forecasts(&pool, &client).await {
                    Ok(n) if n > 0 => {
                        tracing::info!("kalshi forecast poller: {n} forecast row(s) resolved");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("kalshi forecast poller: {e}"),
                }
            }
            if summary.graded > 0 {
                let settings = notification::load_settings();
                let emit_kalshi =
                    notification::kalshi_market_notifications_enabled(&settings);
                let emit_summary =
                    notification::grading_summary_notifications_enabled(&settings);

                if emit_kalshi {
                    for result in &summary.results {
                        let notif_type = if result.outcome == "Win" {
                            NotificationType::KalshiMarketWin
                        } else {
                            NotificationType::KalshiMarketLoss
                        };
                        let emoji = if result.outcome == "Win" { "✅" } else { "❌" };
                        let notif = AppNotification {
                            id: uuid::Uuid::new_v4().to_string(),
                            notification_type: notif_type,
                            title: format!(
                                "{} Kalshi Market Resolved: {}",
                                emoji, result.ticker
                            ),
                            body: format!(
                                "{} — {} (Stake: ${:.2}, PnL: ${:.2})",
                                result.title,
                                result.outcome,
                                result.stake_amount,
                                result.pnl
                            ),
                            player_name: None,
                            game_id: None,
                            prediction_id: Some(result.prediction_id.clone()),
                            created_at: chrono::Utc::now().to_rfc3339(),
                            read: false,
                            dismissed: false,
                        };
                        if let Err(e) = notification::insert_notification(&pool, &notif).await
                        {
                            tracing::warn!("kalshi grade notif persist: {}", e);
                        }
                        let _ = app_handle.emit("notification-new", &notif);
                    }
                }

                if emit_summary {
                    let summary_notif = AppNotification {
                        id: uuid::Uuid::new_v4().to_string(),
                        notification_type: NotificationType::GradingComplete,
                        title: format!(
                            "📊 Kalshi Grading Complete: {} graded",
                            summary.graded
                        ),
                        body: format!(
                            "W: {} | L: {} | PnL: ${:.2}",
                            summary.wins, summary.losses, summary.total_pnl
                        ),
                        player_name: None,
                        game_id: None,
                        prediction_id: None,
                        created_at: chrono::Utc::now().to_rfc3339(),
                        read: false,
                        dismissed: false,
                    };
                    if let Err(e) =
                        notification::insert_notification(&pool, &summary_notif).await
                    {
                        tracing::warn!("kalshi grade summary notif persist: {}", e);
                    }
                    let _ = app_handle.emit("notification-new", &summary_notif);
                }

                tracing::info!(
                    "kalshi auto-grade: {} graded ({}W/{}L, ${:.2})",
                    summary.graded,
                    summary.wins,
                    summary.losses,
                    summary.total_pnl
                );

                let graded = summary.graded;
                let pool_for_ml = pool.clone();
                tauri::async_runtime::spawn(async move {
                    crate::ml_predictor::retrain_after_grading(graded, Some(&pool_for_ml)).await;
                });
            }
        }
    });
}

fn side_label(side: KalshiBetSide) -> String {
    match side {
        KalshiBetSide::Yes => "YES".to_string(),
        KalshiBetSide::No => "NO".to_string(),
        KalshiBetSide::Pass => "PASS".to_string(),
        KalshiBetSide::Unknown => "UNKNOWN".to_string(),
    }
}

pub fn resolved_bet_won(pred: &KalshiPrediction) -> Option<bool> {
    let actual = pred.actual_outcome.as_deref()?;
    let side = parse_bet_side(
        pred.contract_side.as_deref(),
        pred.pick_type.as_deref(),
    );
    bet_won(side, actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pred(side: &str, fair: f64, stake: f64, entry: f64) -> KalshiPrediction {
        KalshiPrediction {
            id: "t".into(),
            ticker: "KXTEST".into(),
            title: "T".into(),
            category: "Economics".into(),
            predicted_probability: fair,
            actual_outcome: None,
            confidence_score: None,
            reasoning: None,
            created_at: String::new(),
            resolved_at: None,
            stake_amount: stake,
            pnl: None,
            pick_type: None,
            price_to_enter: Some(entry),
            market_price_at_entry: None,
            contract_side: Some(side.to_string()),
            edge_points: None,
            fractional_kelly_pct: None,
            recommended_stake_dollars: None,
            risk_flags: None,
            thesis: None,
            data_quality: None,
            decision: None,
        }
    }

    #[test]
    fn yes_below_fifty_wins() {
        assert!(evaluate_bet(&pred("YES", 48.0, 100.0, 0.52), "Yes").unwrap().won);
    }

    #[test]
    fn no_wins_on_no() {
        assert!(evaluate_bet(&pred("NO", 40.0, 100.0, 0.40), "No").unwrap().won);
    }
}