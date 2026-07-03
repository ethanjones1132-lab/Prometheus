//! Portfolio correlation detection and Kelly stake scaling for Kalshi markets.

use super::grading::{parse_bet_side, KalshiBetSide};
use super::models::{KalshiPosition, KalshiPrediction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CorrelationStrength {
    None,
    Category,
    /// Different series that share a common macro/political driver
    /// (e.g. CPI and the Fed rate decision, or the presidential and
    /// senate races). Stronger than a generic same-category match
    /// because the outcomes are driven by the same underlying event.
    Cluster,
    Series,
    Event,
}

impl CorrelationStrength {
    /// Kelly multiplier applied when this correlation is detected.
    pub fn kelly_multiplier(self) -> f64 {
        match self {
            CorrelationStrength::None => 1.0,
            CorrelationStrength::Category => 0.90,
            CorrelationStrength::Cluster => 0.82,
            CorrelationStrength::Series => 0.75,
            CorrelationStrength::Event => 0.50,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CorrelationStrength::None => "independent",
            CorrelationStrength::Category => "same category",
            CorrelationStrength::Cluster => "linked driver",
            CorrelationStrength::Series => "same series",
            CorrelationStrength::Event => "same event",
        }
    }
}

/// Macro/political correlation clusters: groups of distinct Kalshi series whose
/// outcomes are driven by a shared underlying force, so positions across them
/// are correlated even though they share neither a series nor an event key.
///
/// This is the Kalshi-native correlation graph that lifts risk detection above
/// pure ticker-prefix heuristics. The map is keyed by normalized series prefix
/// (uppercased, leading `KX` stripped); add entries as new linked series appear.
struct CorrelationCluster {
    /// Stable cluster id used to compare two series.
    id: &'static str,
    /// Human-readable driver name surfaced in conflict explanations.
    driver: &'static str,
    /// Normalized series prefixes that belong to this cluster.
    series_prefixes: &'static [&'static str],
}

const CORRELATION_CLUSTERS: &[CorrelationCluster] = &[
    CorrelationCluster {
        id: "us-rates-inflation",
        driver: "U.S. rates & inflation regime",
        series_prefixes: &[
            "CPI", "CORECPI", "PCE", "COREPCE", "INFLATION", "FED", "FOMC", "FEDFUNDS",
            "RATEHIKE", "RATECUT", "RATEDECISION", "RATES", "PAYROLLS", "NFP", "JOBS",
            "UNRATE", "JOBLESS", "GDP", "RECESSION",
        ],
    },
    CorrelationCluster {
        id: "us-federal-politics",
        driver: "U.S. federal election / control of government",
        series_prefixes: &[
            "PRES", "PRESIDENT", "POTUS", "SENATE", "HOUSE", "CONGRESS", "PARTYCONTROL",
            "GOVCONTROL", "ELECTION", "ELECTORAL",
        ],
    },
];

/// Normalize a series key for cluster lookup: uppercase, strip a leading `KX`.
fn normalize_series(series: &str) -> String {
    let upper = series.to_ascii_uppercase();
    upper.strip_prefix("KX").unwrap_or(&upper).to_string()
}

/// Return the (cluster id, driver name) a series belongs to, if any.
fn series_cluster(series: &str) -> Option<(&'static str, &'static str)> {
    let norm = normalize_series(series);
    for cluster in CORRELATION_CLUSTERS {
        if cluster
            .series_prefixes
            .iter()
            .any(|p| norm.starts_with(p))
        {
            return Some((cluster.id, cluster.driver));
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioExposure {
    pub ticker: String,
    pub title: String,
    pub category: String,
    pub contract_side: String,
    pub stake_amount: f64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationConflict {
    pub exposure_ticker: String,
    pub exposure_title: String,
    pub strength: String,
    pub kelly_multiplier: f64,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeAdjustment {
    pub kelly_scale: f64,
    pub raw_recommended_stake: f64,
    pub adjusted_recommended_stake: f64,
    pub conflicts: Vec<CorrelationConflict>,
    pub warnings: Vec<String>,
    pub remaining_daily: f64,
    pub remaining_weekly: f64,
    pub bankroll_cap: f64,
}

/// Parse series (KXHIGHNY) and event (KXHIGHNY-25JUN17) keys from a ticker.
pub fn ticker_keys(ticker: &str) -> (String, String) {
    let parts: Vec<&str> = ticker.split('-').collect();
    let series = parts.first().unwrap_or(&ticker).to_string();
    let event = if parts.len() >= 2 {
        format!("{}-{}", parts[0], parts[1])
    } else {
        series.clone()
    };
    (series, event)
}

pub fn correlation_strength(
    target_ticker: &str,
    target_category: &str,
    exposure_ticker: &str,
    exposure_category: &str,
) -> CorrelationStrength {
    if target_ticker == exposure_ticker {
        return CorrelationStrength::Event;
    }
    let (t_series, t_event) = ticker_keys(target_ticker);
    let (e_series, e_event) = ticker_keys(exposure_ticker);
    if t_event == e_event {
        return CorrelationStrength::Event;
    }
    if t_series == e_series {
        return CorrelationStrength::Series;
    }
    if let (Some((t_cluster, _)), Some((e_cluster, _))) =
        (series_cluster(&t_series), series_cluster(&e_series))
    {
        if t_cluster == e_cluster {
            return CorrelationStrength::Cluster;
        }
    }
    if !target_category.is_empty()
        && !exposure_category.is_empty()
        && target_category.eq_ignore_ascii_case(exposure_category)
    {
        return CorrelationStrength::Category;
    }
    CorrelationStrength::None
}

/// Build exposures from pending paper/chat predictions plus live portfolio positions.
/// Build exposures from authenticated Kalshi portfolio positions.
pub fn exposures_from_positions(positions: &[KalshiPosition]) -> Vec<PortfolioExposure> {
    positions
        .iter()
        .filter(|p| p.position != 0)
        .map(|p| {
            let side = if p.position > 0 { "Yes" } else { "No" };
            let stake = (p.market_exposure.unsigned_abs() as f64) / 100.0;
            PortfolioExposure {
                ticker: p.ticker.clone(),
                title: p.ticker.clone(),
                category: String::new(),
                contract_side: side.to_string(),
                stake_amount: stake.max(0.01),
                source: "portfolio".to_string(),
            }
        })
        .collect()
}

pub fn exposures_from_predictions(pending: &[KalshiPrediction]) -> Vec<PortfolioExposure> {
    pending
        .iter()
        .filter(|p| p.actual_outcome.is_none())
        .filter_map(|p| {
            let side = parse_bet_side(
                p.contract_side.as_deref(),
                p.pick_type.as_deref(),
            );
            if side == KalshiBetSide::Pass || side == KalshiBetSide::Unknown {
                return None;
            }
            Some(PortfolioExposure {
                ticker: p.ticker.clone(),
                title: p.title.clone(),
                category: p.category.clone(),
                contract_side: format!("{:?}", side),
                stake_amount: p.stake_amount,
                source: "prediction".to_string(),
            })
        })
        .collect()
}

pub fn compute_stake_adjustment(
    target_ticker: &str,
    target_category: &str,
    target_side: Option<&str>,
    recommended_stake: f64,
    exposures: &[PortfolioExposure],
) -> StakeAdjustment {
    let mut conflicts = Vec::new();
    let mut min_scale = 1.0_f64;
    let mut warnings = Vec::new();

    let target_bet_side = parse_bet_side(target_side, None);

    for exp in exposures {
        if exp.ticker == target_ticker {
            warnings.push(format!(
                "Existing exposure on {} (${:.2} {}) — adding size increases concentration.",
                exp.ticker, exp.stake_amount, exp.contract_side
            ));
            min_scale = min_scale.min(0.85);
            continue;
        }

        let strength = correlation_strength(
            target_ticker,
            target_category,
            &exp.ticker,
            &exp.category,
        );
        if strength == CorrelationStrength::None {
            continue;
        }

        let mult = strength.kelly_multiplier();
        min_scale = min_scale.min(mult);

        let same_direction = exp.contract_side.eq_ignore_ascii_case(&format!("{:?}", target_bet_side));
        let direction_note = if same_direction {
            "same direction"
        } else {
            "opposite direction (partial hedge)"
        };

        // For a cluster link, name the shared macro/political driver.
        let driver_note = if strength == CorrelationStrength::Cluster {
            let (t_series, _) = ticker_keys(target_ticker);
            series_cluster(&t_series)
                .map(|(_, driver)| format!(" via {}", driver))
                .unwrap_or_default()
        } else {
            String::new()
        };

        conflicts.push(CorrelationConflict {
            exposure_ticker: exp.ticker.clone(),
            exposure_title: exp.title.clone(),
            strength: strength.label().to_string(),
            kelly_multiplier: mult,
            explanation: format!(
                "Correlated with active {} position (${:.2} {}){} — {}",
                exp.source, exp.stake_amount, exp.contract_side, driver_note, direction_note
            ),
        });
    }

    if min_scale < 1.0 {
        warnings.push(format!(
            "Kelly stake scaled to {:.0}% due to portfolio correlation (raw ${:.2} → ${:.2}).",
            min_scale * 100.0,
            recommended_stake,
            recommended_stake * min_scale
        ));
    }

    StakeAdjustment {
        kelly_scale: min_scale,
        raw_recommended_stake: recommended_stake,
        adjusted_recommended_stake: recommended_stake * min_scale,
        conflicts,
        warnings,
        remaining_daily: 0.0,
        remaining_weekly: 0.0,
        bankroll_cap: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_correlation_scales_to_half() {
        let adj = compute_stake_adjustment(
            "KXHIGHNY-25JUN17-T75",
            "Weather",
            Some("YES"),
            100.0,
            &[PortfolioExposure {
                ticker: "KXHIGHNY-25JUN17-T80".into(),
                title: "High > 80".into(),
                category: "Weather".into(),
                contract_side: "Yes".into(),
                stake_amount: 50.0,
                source: "prediction".into(),
            }],
        );
        assert!((adj.kelly_scale - 0.5).abs() < 0.01);
        assert!((adj.adjusted_recommended_stake - 50.0).abs() < 0.01);
    }

    #[test]
    fn macro_cluster_links_cpi_and_fed() {
        // CPI and the Fed rate decision are different series in different events
        // but share the same macro driver — should scale to the Cluster multiplier.
        let strength = correlation_strength("KXCPIYOY-25JUL", "Economics", "KXFED-25JUL", "Economics");
        assert_eq!(strength, CorrelationStrength::Cluster);

        let adj = compute_stake_adjustment(
            "KXCPIYOY-25JUL",
            "Economics",
            Some("YES"),
            100.0,
            &[PortfolioExposure {
                ticker: "KXFED-25JUL-T25".into(),
                title: "Fed hikes 25bp".into(),
                category: "Economics".into(),
                contract_side: "Yes".into(),
                stake_amount: 40.0,
                source: "prediction".into(),
            }],
        );
        assert!((adj.kelly_scale - 0.82).abs() < 0.01);
        assert_eq!(adj.conflicts.len(), 1);
        assert!(adj.conflicts[0].explanation.contains("rates & inflation"));
    }

    #[test]
    fn politics_cluster_links_presidential_and_senate() {
        let strength = correlation_strength("KXPRES-28", "Politics", "KXSENATE-28-TX", "Politics");
        assert_eq!(strength, CorrelationStrength::Cluster);
    }

    #[test]
    fn unrelated_series_in_different_clusters_are_independent() {
        // CPI (macro) vs presidential (politics): no series, event, or cluster overlap,
        // and different categories — should be independent.
        let strength = correlation_strength("KXCPIYOY-25JUL", "Economics", "KXPRES-28", "Politics");
        assert_eq!(strength, CorrelationStrength::None);
    }
}