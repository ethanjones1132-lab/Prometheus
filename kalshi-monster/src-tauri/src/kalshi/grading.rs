//! Contract-side-aware Kalshi grading and binary PnL.

use super::models::KalshiPrediction;
use crate::kalshi::client::KalshiClient;
use crate::kalshi::models::{KalshiGradingResult, KalshiGradingSummary};
use crate::predictions::tracker::PredictionTracker;
use std::collections::HashMap;

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

pub fn spawn_auto_grade_task(
    kalshi: std::sync::Arc<tokio::sync::Mutex<KalshiClient>>,
    tracker: std::sync::Arc<tokio::sync::Mutex<PredictionTracker>>,
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
            if pending_count == 0 {
                continue;
            }
            let summary = {
                let t = tracker.lock().await;
                let client = kalshi.lock().await;
                match grade_pending_predictions(&t, &client).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("kalshi auto-grade: {}", e);
                        continue;
                    }
                }
            };
            if summary.graded > 0 {
                tracing::info!(
                    "kalshi auto-grade: {} graded ({}W/{}L, ${:.2})",
                    summary.graded,
                    summary.wins,
                    summary.losses,
                    summary.total_pnl
                );
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