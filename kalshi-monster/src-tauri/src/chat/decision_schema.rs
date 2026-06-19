//! Professional Decision Schema for Kalshi Market Analysis
//!
//! Every market analysis output should support this structured format,
//! enabling the frontend to render trade tickets, journal entries,
//! and risk alerts with full data fidelity.

use serde::{Deserialize, Serialize};

/// Professional trade decision for a Kalshi prediction market.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct KalshiTradeDecision {
    /// Kalshi market ticker (e.g. "KXEVENT-FED-25DEC")
    pub ticker: String,
    /// Human-readable market title
    pub market_title: String,
    /// Market category: Sports, Politics, Economics, Crypto, Finance, Weather, Other
    pub category: String,
    /// Which side of the contract: YES, NO, or PASS
    pub contract_side: ContractSide,
    /// Current market price for the selected side (0.0–1.0)
    pub market_price_pct: f64,
    /// Model's fair probability estimate (0.0–100.0)
    pub fair_probability_pct: f64,
    /// Edge in percentage points (fair_probability – market_price * 100)
    pub edge_points: f64,
    /// Bid-ask spread in cents
    pub spread_cents: f64,
    /// Liquidity score: 0–100 (higher = deeper book)
    pub liquidity_score: f64,
    /// EV per contract in cents (expected value of one share)
    pub ev_per_contract_cents: f64,
    /// EV as a percentage ROI
    pub ev_roi_pct: f64,
    /// Raw Kelly percentage (unbounded, can be >100%)
    pub raw_kelly_pct: f64,
    /// Recommended fractional Kelly percentage (conservative)
    pub fractional_kelly_pct: f64,
    /// Recommended stake in dollars
    pub recommended_stake_dollars: f64,
    /// Maximum position size in dollars
    pub max_position_dollars: f64,
    /// Final decision
    pub decision: DecisionAction,
    /// Confidence tier
    pub confidence_tier: ConfidenceTier,
    /// Calibrated thesis (2–3 sentences)
    pub thesis: String,
    /// Supporting evidence bullets
    pub evidence: Vec<String>,
    /// Risk flags identified
    pub risk_flags: Vec<RiskFlag>,
    /// Quality rating of the data behind this decision
    pub data_quality: DataQuality,
    /// Price at which to enter the position
    pub price_to_enter: f64,
}

/// Side of the binary contract
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum ContractSide {
    YES,
    NO,
    #[default]
    PASS,
}

/// Final recommended action
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum DecisionAction {
    /// Execute the trade — the edge justifies action
    TAKE,
    /// Monitor — not enough edge or data to act
    WATCH,
    /// Skip — negative EV or excessive risk
    #[default]
    PASS,
}

/// Confidence tier based on model certainty and data quality
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum ConfidenceTier {
    /// Strong conviction + excellent data quality
    High,
    /// Moderate conviction + good data quality
    Medium,
    /// Weak conviction or incomplete data
    Low,
    /// No confidence — default for PASS
    #[default]
    None,
}

/// Risk flags that can downgrade or invalidate a trade
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RiskFlag {
    /// Bid-ask spread is wider than the estimated edge
    SpreadExceedsEdge,
    /// Insufficient liquidity for the recommended stake
    InsufficientLiquidity,
    /// High correlation with an existing position
    CorrelatedExposure,
    /// Market uses provisional settlement rules
    ProvisionalSettlement,
    /// Market can close before expected
    EarlyCloseRisk,
    /// Extreme probability (>90% or <10%)
    ExtremeProbability,
    /// Resolution criteria are ambiguous
    AmbiguousResolution,
    /// Data is stale or incomplete
    StaleData,
    /// Position would exceed maximum portfolio allocation
    ConcentrationRisk,
    /// Other unspecified risk
    Other(String),
}

/// Quality of the data used to make this decision
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum DataQuality {
    /// Real-time Kalshi API data with live orderbook
    Live,
    /// Cached data < 60 seconds old
    Fresh,
    /// Cached data 1–5 minutes old
    Stale,
    /// No direct market data — reasoning from base rates and news only
    #[default]
    Inferential,
    /// Speculative — very limited data
    Speculative,
}

impl KalshiTradeDecision {
    /// Create a new decision with sensible defaults
    pub fn new(ticker: &str, market_title: &str) -> Self {
        Self {
            ticker: ticker.to_string(),
            market_title: market_title.to_string(),
            category: "Other".to_string(),
            contract_side: ContractSide::PASS,
            market_price_pct: 0.0,
            fair_probability_pct: 50.0,
            edge_points: 0.0,
            spread_cents: 0.0,
            liquidity_score: 0.0,
            ev_per_contract_cents: 0.0,
            ev_roi_pct: 0.0,
            raw_kelly_pct: 0.0,
            fractional_kelly_pct: 0.0,
            recommended_stake_dollars: 0.0,
            max_position_dollars: 0.0,
            decision: DecisionAction::PASS,
            confidence_tier: ConfidenceTier::None,
            thesis: String::new(),
            evidence: Vec::new(),
            risk_flags: Vec::new(),
            data_quality: DataQuality::Inferential,
            price_to_enter: 0.0,
        }
    }

    /// Compute edge, EV, and Kelly sizing from market price and fair probability.
    /// Call this after setting market_price_pct and fair_probability_pct.
    pub fn compute(&mut self, bankroll_dollars: f64, kelly_fraction: f64) {
        let market_price = self.market_price_pct / 100.0;
        let fair_prob = self.fair_probability_pct / 100.0;

        if market_price <= 0.0 || market_price >= 1.0 || fair_prob <= 0.0 || fair_prob >= 1.0 {
            self.edge_points = 0.0;
            self.ev_roi_pct = 0.0;
            self.raw_kelly_pct = 0.0;
            self.fractional_kelly_pct = 0.0;
            self.recommended_stake_dollars = 0.0;
            return;
        }

        // Edge in percentage points for the selected side
        self.edge_points = if self.contract_side == ContractSide::YES {
            (fair_prob - market_price) * 100.0
        } else if self.contract_side == ContractSide::NO {
            (market_price - fair_prob) * 100.0
        } else {
            0.0
        };

        // EV per contract
        if self.contract_side == ContractSide::YES {
            self.ev_per_contract_cents = (fair_prob - market_price) * 100.0;
            self.ev_roi_pct = ((fair_prob / market_price) - 1.0) * 100.0;
        } else if self.contract_side == ContractSide::NO {
            let no_price = 1.0 - market_price;
            let no_fair = 1.0 - fair_prob;
            self.ev_per_contract_cents = (no_fair - no_price) * 100.0;
            self.ev_roi_pct = ((no_fair / no_price) - 1.0) * 100.0;
        } else {
            self.ev_per_contract_cents = 0.0;
            self.ev_roi_pct = 0.0;
        }

        // Kelly Criterion: f* = (p * b - q) / b
        let raw_kelly = if self.contract_side == ContractSide::YES {
            let p = fair_prob;
            let q = 1.0 - p;
            let b = (1.0 - market_price) / market_price;
            if b > 0.0 {
                (p * b - q) / b
            } else {
                0.0
            }
        } else if self.contract_side == ContractSide::NO {
            let p = 1.0 - fair_prob;
            let q = 1.0 - p;
            let b = market_price / (1.0 - market_price);
            if b > 0.0 {
                (p * b - q) / b
            } else {
                0.0
            }
        } else {
            0.0
        };

        self.raw_kelly_pct = raw_kelly.max(0.0) * 100.0;
        self.fractional_kelly_pct = self.raw_kelly_pct * kelly_fraction;
        self.recommended_stake_dollars = bankroll_dollars * (self.fractional_kelly_pct / 100.0);

        // Liquidity score: simplistic scoring based on volume
        self.liquidity_score = ((self.liquidity_score / 50000.0) * 100.0).min(100.0);

        // Max position: cap at 5% of bankroll or liquidity limit
        self.max_position_dollars = (bankroll_dollars * 0.05).min(self.recommended_stake_dollars);
    }

    /// Compute with isotonic calibration and portfolio correlation Kelly scaling.
    pub fn compute_risk_adjusted(
        &mut self,
        bankroll_dollars: f64,
        kelly_fraction: f64,
        kelly_scale: f64,
        apply_calibrator: bool,
    ) {
        if apply_calibrator {
            let cal = crate::analysis::calibration::calibrate_yes_probability_pct(
                self.fair_probability_pct,
            );
            if cal.applied {
                self.fair_probability_pct = cal.calibrated_pct;
            }
        }
        self.compute(bankroll_dollars, kelly_fraction);
        let scale = kelly_scale.clamp(0.0, 1.0);
        if scale < 1.0 {
            self.fractional_kelly_pct *= scale;
            self.recommended_stake_dollars *= scale;
            self.max_position_dollars = self.max_position_dollars.min(self.recommended_stake_dollars);
            if !self.risk_flags.contains(&RiskFlag::CorrelatedExposure) {
                self.risk_flags.push(RiskFlag::CorrelatedExposure);
            }
        }
    }

    /// Return true if the decision passes all risk checks
    pub fn is_actionable(&self) -> bool {
        if self.decision != DecisionAction::TAKE {
            return false;
        }
        if !self.risk_flags.is_empty() {
            // Any risk flag except StaleData might be acceptable
            return self.risk_flags.iter().all(|f| matches!(f, RiskFlag::StaleData));
        }
        true
    }

    /// Generate the AI prompt fragment describing this decision structure
    pub fn prompt_schema() -> String {
        String::from(
            r#"## KALSHI TRADE DECISION SCHEMA

Every market analysis must output a JSON block with the following fields FIRST:

{
  "ticker": "KXEVENT-FED-25DEC",
  "market_title": "Will the Fed raise rates by 25bp?",
  "category": "Economics",
  "contract_side": "YES",
  "market_price_pct": 55.0,
  "fair_probability_pct": 62.0,
  "edge_points": 7.0,
  "spread_cents": 3.0,
  "liquidity_score": 75.0,
  "ev_per_contract_cents": 7.0,
  "ev_roi_pct": 12.7,
  "raw_kelly_pct": 22.4,
  "fractional_kelly_pct": 5.6,
  "recommended_stake_dollars": 56.0,
  "max_position_dollars": 50.0,
  "decision": "TAKE",
  "confidence_tier": "High",
  "thesis": "The market underweights the persistence of core inflation relative to recent FOMC rhetoric.",
  "evidence": [
    "Core PCE exceeded expectations for 3 consecutive months",
    "FOMC dot plot shifted hawkish in June",
    "Market pricing: 55c vs model: 62c"
  ],
  "risk_flags": ["EarlyCloseRisk"],
  "data_quality": "Live",
  "price_to_enter": 0.55
}

RULES:
- "decision" must be "TAKE", "WATCH", or "PASS".
- "contract_side" must be "YES", "NO", or "PASS".
- "confidence_tier" must be "High", "Medium", "Low", or "None".
- "data_quality" must be "Live", "Fresh", "Stale", "Inferential", or "Speculative".
- "risk_flags" can include: SpreadExceedsEdge, InsufficientLiquidity, CorrelatedExposure, ProvisionalSettlement, EarlyCloseRisk, ExtremeProbability, AmbiguousResolution, StaleData, ConcentrationRisk.
- JSON must be valid. No trailing commas. Place it FIRST in the response.

After the JSON, provide a concise readable summary:
- DECISION: [TAKE/WATCH/PASS] [YES/NO] at [price]
- PRICE VS FAIR: [market]% vs [fair]%
- EDGE: [edge points] pts, [EV ROI]% EV ROI
- SIZE: [raw Kelly]% raw Kelly, [fractional Kelly]% recommended
- WHY: [thesis]
- RISK CONTROL: [key risk flags and invalidation conditions]
"#
        )
    }
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kelly_calculation() {
        let mut decision = KalshiTradeDecision::new("KX-FED-25DEC", "Fed Rate Decision");
        decision.market_price_pct = 55.0;
        decision.fair_probability_pct = 62.0;
        decision.contract_side = ContractSide::YES;
        decision.liquidity_score = 50000.0; // Will be normalized to 100
        decision.compute(1000.0, 0.25);

        assert!((decision.edge_points - 7.0).abs() < 0.01);
        assert!(decision.ev_roi_pct > 0.0);
        assert!(decision.raw_kelly_pct > 0.0);
        assert!(decision.fractional_kelly_pct > 0.0);
        assert!(decision.recommended_stake_dollars > 0.0);
    }

    #[test]
    fn test_negative_ev_passes() {
        let mut decision = KalshiTradeDecision::new("KX-FAKE", "Fake Market");
        decision.market_price_pct = 80.0;
        decision.fair_probability_pct = 70.0;
        decision.contract_side = ContractSide::YES;
        decision.compute(1000.0, 0.25);

        // Edge is negative — should not recommend a stake
        assert!(decision.edge_points < 0.0);
        assert!(decision.recommended_stake_dollars == 0.0);
    }

    #[test]
    fn test_spread_exceeds_edge_flag() {
        let risk = RiskFlag::SpreadExceedsEdge;
        match risk {
            RiskFlag::SpreadExceedsEdge => {}
            _ => panic!("Expected SpreadExceedsEdge"),
        }
    }

    #[test]
    fn test_decision_enum_serialization() {
        let take = DecisionAction::TAKE;
        let json = serde_json::to_string(&take).unwrap();
        assert_eq!(json, "\"TAKE\"");

        let parsed: DecisionAction = serde_json::from_str("\"PASS\"").unwrap();
        assert_eq!(parsed, DecisionAction::PASS);
    }

    #[test]
    fn test_is_actionable_with_risk_flags() {
        let mut decision = KalshiTradeDecision::new("KX-TEST", "Test Market");
        decision.decision = DecisionAction::TAKE;
        assert!(decision.is_actionable());

        decision.risk_flags.push(RiskFlag::SpreadExceedsEdge);
        assert!(!decision.is_actionable()); // Now has a blocking flag
    }

    #[test]
    fn test_contract_side_no_ev() {
        let mut decision = KalshiTradeDecision::new("KX-TEST", "Test");
        decision.market_price_pct = 60.0;
        decision.fair_probability_pct = 40.0;
        decision.contract_side = ContractSide::NO;
        decision.compute(1000.0, 0.25);

        assert!((decision.edge_points - 20.0).abs() < 0.01);
        assert!((decision.raw_kelly_pct - 33.33).abs() < 0.05);
        assert!((decision.fractional_kelly_pct - 8.33).abs() < 0.05);
        assert!((decision.recommended_stake_dollars - 83.33).abs() < 0.5);
        assert!(decision.ev_roi_pct > 0.0);
    }
}
