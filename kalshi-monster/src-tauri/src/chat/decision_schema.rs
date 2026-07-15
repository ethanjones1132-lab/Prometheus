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
    /// Selected-side market price as percent of $1 (0–100). Prefer writing dollars in prompts;
    /// post-process coerces 0–1 dollar inputs to this unit.
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
    /// Recommended fractional Kelly percentage of bankroll (capped by app policy)
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
    /// Entry price in dollars per contract (0–1). Never cents.
    pub price_to_enter: f64,
    /// Whether the model's fair probability diverges significantly from market implied prob
    pub model_disagreement: bool,
}

/// Coerce a contract price that may be dollars (0–1), cents, or percent into **dollars** in [0, 1].
///
/// Kalshi binary contracts cost between $0.01 and $0.99 typically. LLM outputs often mix
/// units (0.55 dollars vs 55 cents vs 55 percent). Values in (0, 1] are treated as dollars;
/// values in (1, 100] as cents/percent.
pub fn coerce_price_to_dollars(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 0.0;
    }
    if raw <= 1.0 {
        raw
    } else if raw <= 100.0 {
        raw / 100.0
    } else {
        (raw / 100.0).clamp(0.0, 1.0)
    }
}

/// Normalize `market_price_pct` + `price_to_enter` together so sub-1% markets
/// (e.g. market_price_pct=0.45 meaning **0.45%**, price_to_enter=0.005 dollars)
/// are not misread as $0.45.
///
/// Returns (market_price_pct 0–100, price_to_enter 0–1 dollars).
pub fn coerce_market_and_entry(market_price_pct: f64, price_to_enter: f64) -> (f64, f64) {
    let enter = if price_to_enter > 0.0 {
        coerce_price_to_dollars(price_to_enter)
    } else {
        0.0
    };

    // Explicit dollar entry on (0,1]: use it to disambiguate market_price_pct.
    if enter > 0.0 && enter <= 1.0 {
        if market_price_pct > 1.0 {
            return (market_price_pct.clamp(0.0, 100.0), enter);
        }
        if market_price_pct <= 0.0 {
            return ((enter * 100.0).clamp(0.0, 100.0), enter);
        }
        // Both in (0,1]: if market ≈ enter → market was dollars; else market is percent (long-shot).
        let rel = (market_price_pct - enter).abs() / enter.max(1e-9);
        if rel < 0.25 {
            return ((market_price_pct * 100.0).clamp(0.0, 100.0), enter);
        }
        return (market_price_pct.clamp(0.0, 100.0), enter);
    }

    // No usable entry — classic coerce
    let market_dollars = coerce_price_to_dollars(market_price_pct);
    let enter2 = if enter > 0.0 {
        enter
    } else {
        market_dollars
    };
    (
        (market_dollars * 100.0).clamp(0.0, 100.0),
        enter2.clamp(0.0, 1.0),
    )
}

/// Coerce a probability that may be 0–1 or 0–100 into **percent** in [0, 100].
pub fn coerce_probability_to_pct(raw: f64) -> f64 {
    if !raw.is_finite() {
        return 50.0;
    }
    if raw < 0.0 {
        return 0.0;
    }
    if raw <= 1.0 {
        raw * 100.0
    } else {
        raw.min(100.0)
    }
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
    /// Position would exceed configured daily/weekly bankroll cap
    BankrollLimitExceeded,
    /// Model's fair probability diverges significantly from market implied probability
    ModelDisagreement,
    /// Tape says market is settled / closed — TAKE forbidden
    MarketSettledOrClosed,
    /// Circuit breaker (§6.4) active — stake scaled or trading blocked
    CircuitBreakerActive,
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
            model_disagreement: false,
        }
    }

    /// Normalize price/probability units then clamp Kelly & stake to app policy **before** TAKE is actionable.
    ///
    /// Units after this call:
    /// - `market_price_pct`: 0–100 (percent of $1)
    /// - `fair_probability_pct`: 0–100
    /// - `price_to_enter`: 0–1 dollars
    /// - `fractional_kelly_pct` ≤ `max_bet_pct * 100` and ≤ `raw * kelly_fraction` when raw is known
    /// - `recommended_stake_dollars` ≤ `bankroll * max_bet_pct`
    pub fn sanitize_units_and_caps(
        &mut self,
        bankroll_dollars: f64,
        kelly_fraction: f64,
        max_bet_pct: f64,
    ) {
        // Joint unit normalize (handles sub-1% long-shots correctly).
        let (mkt_pct, enter) =
            coerce_market_and_entry(self.market_price_pct, self.price_to_enter);
        self.market_price_pct = mkt_pct;
        self.price_to_enter = enter;
        self.fair_probability_pct = coerce_probability_to_pct(self.fair_probability_pct);

        let kelly_frac = if kelly_fraction > 0.0 && kelly_fraction <= 1.0 {
            kelly_fraction
        } else {
            0.25
        };
        let max_bet = if max_bet_pct > 0.0 && max_bet_pct <= 1.0 {
            max_bet_pct
        } else {
            0.05
        };
        let max_frac_kelly_pct = max_bet * 100.0;

        // Re-derive fractional from raw when raw is present and fractional looks noncompliant
        // (e.g. LLM wrote fractional_kelly_pct ≈ 99.8 for a long-shot).
        if self.raw_kelly_pct > 0.0 {
            let policy_frac = self.raw_kelly_pct * kelly_frac;
            if self.fractional_kelly_pct <= 0.0 || self.fractional_kelly_pct > policy_frac + 0.01 {
                self.fractional_kelly_pct = policy_frac;
            }
        }

        let mut capped = false;
        if self.fractional_kelly_pct > max_frac_kelly_pct {
            self.fractional_kelly_pct = max_frac_kelly_pct;
            capped = true;
        }
        // Absolute safety rail: never surface >25% bankroll as "fractional Kelly" on a TAKE ticket.
        if self.fractional_kelly_pct > 25.0 {
            self.fractional_kelly_pct = 25.0_f64.min(max_frac_kelly_pct);
            capped = true;
        }

        if bankroll_dollars > 0.0 {
            let max_stake = bankroll_dollars * max_bet;
            if self.recommended_stake_dollars <= 0.0
                && self.decision == DecisionAction::TAKE
                && self.fractional_kelly_pct > 0.0
            {
                self.recommended_stake_dollars =
                    bankroll_dollars * (self.fractional_kelly_pct / 100.0);
            }
            if self.recommended_stake_dollars > max_stake {
                self.recommended_stake_dollars = max_stake;
                capped = true;
            }
            if self.max_position_dollars <= 0.0 || self.max_position_dollars > max_stake {
                self.max_position_dollars = max_stake.min(self.recommended_stake_dollars.max(0.0));
            } else {
                self.max_position_dollars = self
                    .max_position_dollars
                    .min(max_stake)
                    .min(self.recommended_stake_dollars.max(self.max_position_dollars));
            }
        }

        if capped {
            if !self.risk_flags.contains(&RiskFlag::BankrollLimitExceeded) {
                self.risk_flags.push(RiskFlag::BankrollLimitExceeded);
            }
            if self.decision == DecisionAction::TAKE {
                let note = format!(
                    "[sizing capped: fractional Kelly ≤ {:.1}% bankroll, stake ≤ bankroll×{:.0}%]",
                    max_frac_kelly_pct,
                    max_bet * 100.0
                );
                if !self.thesis.contains("[sizing capped") {
                    if !self.thesis.is_empty() {
                        self.thesis.push(' ');
                    }
                    self.thesis.push_str(&note);
                }
            }
        }
    }

    /// True when the ticker is a schema placeholder or otherwise not a real market id.
    /// Used to reject LLM copy-paste of the example ticket (e.g. `KXEVENT-TICKER`).
    pub fn is_placeholder_ticker(ticker: &str) -> bool {
        let t = ticker.trim().to_uppercase();
        if t.is_empty() {
            return true;
        }
        // Exact / common schema placeholders observed in production logs.
        if t == "KXEVENT-TICKER"
            || t == "KX-EVENT-TICKER"
            || t == "KXEVENT"
            || t == "TICKER"
            || t == "KX-TICKER"
            || t == "KXTEST"
            || t.ends_with("-TICKER")
            || t.contains("PLACEHOLDER")
            || t.contains("EXAMPLE")
        {
            return true;
        }
        // Must look like a Kalshi-style ticker (starts with KX and has a hyphenated body).
        if !t.starts_with("KX") || !t.contains('-') || t.len() < 6 {
            return true;
        }
        false
    }

    /// Policy rails that improve prediction quality **without changing Kelly/EV formulas**.
    ///
    /// - Rejects placeholder tickers
    /// - Forces PASS/WATCH when spread exceeds |edge|
    /// - Caps confidence and blocks extreme longshot multiplies without live data
    /// - Zeroes stake when TAKE is demoted
    pub fn enforce_prediction_quality_rails(&mut self) {
        // Placeholder / invented tickers are never actionable.
        if Self::is_placeholder_ticker(&self.ticker) {
            self.decision = DecisionAction::PASS;
            self.contract_side = ContractSide::PASS;
            self.recommended_stake_dollars = 0.0;
            self.max_position_dollars = 0.0;
            self.fractional_kelly_pct = 0.0;
            self.raw_kelly_pct = 0.0;
            self.confidence_tier = ConfidenceTier::None;
            if !self.risk_flags.contains(&RiskFlag::StaleData) {
                self.risk_flags.push(RiskFlag::StaleData);
            }
            let note = "[quality rail: placeholder/invalid ticker — forced PASS]";
            if !self.thesis.contains("[quality rail: placeholder") {
                if !self.thesis.is_empty() {
                    self.thesis.push(' ');
                }
                self.thesis.push_str(note);
            }
            return;
        }

        // High confidence is reserved for Live/Fresh books.
        if matches!(
            self.confidence_tier,
            ConfidenceTier::High
        ) && matches!(
            self.data_quality,
            DataQuality::Inferential | DataQuality::Speculative | DataQuality::Stale
        ) {
            self.confidence_tier = ConfidenceTier::Low;
            let note = "[quality rail: High confidence requires Live/Fresh data — demoted]";
            if !self.thesis.contains("[quality rail: High confidence") {
                if !self.thesis.is_empty() {
                    self.thesis.push(' ');
                }
                self.thesis.push_str(note);
            }
        }

        // Spread vs edge friction (compare absolute edge points to spread cents).
        let abs_edge = self.edge_points.abs();
        if self.spread_cents > 0.0 && abs_edge > 0.0 && self.spread_cents > abs_edge {
            if !self.risk_flags.contains(&RiskFlag::SpreadExceedsEdge) {
                self.risk_flags.push(RiskFlag::SpreadExceedsEdge);
            }
            if self.decision == DecisionAction::TAKE {
                self.decision = DecisionAction::PASS;
                self.recommended_stake_dollars = 0.0;
                self.max_position_dollars = 0.0;
                self.fractional_kelly_pct = 0.0;
                self.raw_kelly_pct = 0.0;
                self.confidence_tier = ConfidenceTier::None;
                let note = "[quality rail: spread exceeds edge — forced PASS]";
                if !self.thesis.contains("[quality rail: spread exceeds") {
                    if !self.thesis.is_empty() {
                        self.thesis.push(' ');
                    }
                    self.thesis.push_str(note);
                }
            }
        }

        // Extreme longshot multiple: market <5% and fair >> market without live tape.
        // Does not rewrite fair_probability (math untouched) — only demotes action.
        let mkt = self.market_price_pct;
        let fair = self.fair_probability_pct;
        if mkt > 0.0 && mkt < 5.0 && fair > mkt * 5.0 {
            if !self.risk_flags.contains(&RiskFlag::ExtremeProbability) {
                self.risk_flags.push(RiskFlag::ExtremeProbability);
            }
            if !self.risk_flags.contains(&RiskFlag::ModelDisagreement) {
                self.risk_flags.push(RiskFlag::ModelDisagreement);
            }
            let weak_data = matches!(
                self.data_quality,
                DataQuality::Inferential | DataQuality::Speculative | DataQuality::Stale
            );
            if self.decision == DecisionAction::TAKE && weak_data {
                self.decision = DecisionAction::WATCH;
                self.recommended_stake_dollars = 0.0;
                self.max_position_dollars = 0.0;
                self.fractional_kelly_pct = 0.0;
                if matches!(self.confidence_tier, ConfidenceTier::High | ConfidenceTier::Medium) {
                    self.confidence_tier = ConfidenceTier::Low;
                }
                let note = "[quality rail: longshot fair≫market without Live data — demoted to WATCH]";
                if !self.thesis.contains("[quality rail: longshot") {
                    if !self.thesis.is_empty() {
                        self.thesis.push(' ');
                    }
                    self.thesis.push_str(note);
                }
            }
        }

        // Very wide spreads with no claimed liquidity → never TAKE.
        if self.decision == DecisionAction::TAKE
            && self.spread_cents >= 25.0
            && self.liquidity_score < 20.0
        {
            if !self.risk_flags.contains(&RiskFlag::InsufficientLiquidity) {
                self.risk_flags.push(RiskFlag::InsufficientLiquidity);
            }
            self.decision = DecisionAction::PASS;
            self.recommended_stake_dollars = 0.0;
            self.max_position_dollars = 0.0;
            self.fractional_kelly_pct = 0.0;
            self.raw_kelly_pct = 0.0;
            self.confidence_tier = ConfidenceTier::None;
            let note = "[quality rail: wide illiquid book — forced PASS]";
            if !self.thesis.contains("[quality rail: wide illiquid") {
                if !self.thesis.is_empty() {
                    self.thesis.push(' ');
                }
                self.thesis.push_str(note);
            }
        }
    }

    /// Hard rail: SETTLED/CLOSED tape → never keep TAKE. Zeros stake and rewrites decision.
    pub fn enforce_settlement_gate(&mut self, gate: &crate::chat::market_gate::MarketGate) {
        use crate::chat::market_gate::MarketGate;
        if gate.allows_take() {
            return;
        }
        let was_take = self.decision == DecisionAction::TAKE;
        self.decision = DecisionAction::PASS;
        self.contract_side = ContractSide::PASS;
        self.recommended_stake_dollars = 0.0;
        self.max_position_dollars = 0.0;
        self.fractional_kelly_pct = 0.0;
        self.raw_kelly_pct = 0.0;
        self.confidence_tier = ConfidenceTier::None;
        if !self.risk_flags.contains(&RiskFlag::MarketSettledOrClosed) {
            self.risk_flags.push(RiskFlag::MarketSettledOrClosed);
        }
        let reason = match gate {
            MarketGate::Settled { reason, result } => {
                let r = result
                    .as_deref()
                    .map(|x| format!(" result={x}"))
                    .unwrap_or_default();
                format!("GATE=SETTLED{r}: {reason}")
            }
            MarketGate::Closed { reason } => format!("GATE=CLOSED: {reason}"),
            MarketGate::Open => return,
        };
        let note = format!("[settlement rail: forced PASS — {reason}]");
        if was_take || !self.thesis.contains("[settlement rail") {
            if !self.thesis.is_empty() {
                self.thesis.push(' ');
            }
            if !self.thesis.contains("[settlement rail") {
                self.thesis.push_str(&note);
            }
        }
    }

    /// Compute edge, EV, and Kelly sizing from market price and fair probability.
    /// Uses default max bet of 5% of bankroll.
    pub fn compute(&mut self, bankroll_dollars: f64, kelly_fraction: f64) {
        self.compute_with_policy(bankroll_dollars, kelly_fraction, 0.05);
    }

    /// Compute edge/EV/Kelly then enforce unit normalization and sizing caps.
    pub fn compute_with_policy(
        &mut self,
        bankroll_dollars: f64,
        kelly_fraction: f64,
        max_bet_pct: f64,
    ) {
        self.sanitize_units_and_caps(bankroll_dollars, kelly_fraction, max_bet_pct);

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

        let max_bet = if max_bet_pct > 0.0 && max_bet_pct <= 1.0 {
            max_bet_pct
        } else {
            0.05
        };

        self.raw_kelly_pct = raw_kelly.max(0.0) * 100.0;
        self.fractional_kelly_pct = self.raw_kelly_pct * kelly_fraction;
        self.recommended_stake_dollars = bankroll_dollars * (self.fractional_kelly_pct / 100.0);

        // Liquidity score: simplistic scoring based on volume
        self.liquidity_score = ((self.liquidity_score / 50000.0) * 100.0).min(100.0);

        // Max position: policy max bet of bankroll
        self.max_position_dollars =
            (bankroll_dollars * max_bet).min(self.recommended_stake_dollars);

        // Model disagreement detection: flag when fair prob diverges significantly from market
        let market_implied_pct = self.market_price_pct;
        let divergence = (self.fair_probability_pct - market_implied_pct).abs();
        self.model_disagreement = divergence >= 15.0;
        if self.model_disagreement && !self.risk_flags.contains(&RiskFlag::ModelDisagreement) {
            self.risk_flags.push(RiskFlag::ModelDisagreement);
        }

        // Re-apply hard caps after Kelly math (TAKE must never surface uncapped size)
        self.sanitize_units_and_caps(bankroll_dollars, kelly_fraction, max_bet);
    }

    /// Compute with isotonic calibration and portfolio correlation Kelly scaling.
    pub fn compute_risk_adjusted(
        &mut self,
        bankroll_dollars: f64,
        kelly_fraction: f64,
        kelly_scale: f64,
        apply_calibrator: bool,
    ) {
        self.compute_risk_adjusted_with_policy(
            bankroll_dollars,
            kelly_fraction,
            kelly_scale,
            apply_calibrator,
            0.05,
        );
    }

    /// Like [`Self::compute_risk_adjusted`] with an explicit max-bet fraction of bankroll.
    pub fn compute_risk_adjusted_with_policy(
        &mut self,
        bankroll_dollars: f64,
        kelly_fraction: f64,
        kelly_scale: f64,
        apply_calibrator: bool,
        max_bet_pct: f64,
    ) {
        if apply_calibrator {
            let cal = crate::analysis::calibration::calibrate_yes_probability_pct(
                self.fair_probability_pct,
            );
            if cal.applied {
                self.fair_probability_pct = cal.calibrated_pct;
            }
        }
        self.compute_with_policy(bankroll_dollars, kelly_fraction, max_bet_pct);
        let scale = kelly_scale.clamp(0.0, 1.0);
        if scale < 1.0 {
            self.fractional_kelly_pct *= scale;
            self.recommended_stake_dollars *= scale;
            self.max_position_dollars = self.max_position_dollars.min(self.recommended_stake_dollars);
            if !self.risk_flags.contains(&RiskFlag::CorrelatedExposure) {
                self.risk_flags.push(RiskFlag::CorrelatedExposure);
            }
        }
        self.sanitize_units_and_caps(bankroll_dollars, kelly_fraction, max_bet_pct);
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
  "ticker": "KXFED-25DEC-H25",
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
- "risk_flags" can include: SpreadExceedsEdge, InsufficientLiquidity, CorrelatedExposure, ProvisionalSettlement, EarlyCloseRisk, ExtremeProbability, AmbiguousResolution, StaleData, ConcentrationRisk, ModelDisagreement, MarketSettledOrClosed.
- PRICE UNITS (critical):
  - market_price_pct = selected-side cost as percent of $1 (55.0 means $0.55 / 55¢). Do NOT write raw cents as if they were percent.
  - price_to_enter = dollars per contract on [0, 1] (0.55 means 55¢). Never cents (55) and never "0.1¢" style.
  - fair_probability_pct = probability percent on [0, 100].
- KELLY POLICY: prefer quarter-Kelly. fractional_kelly_pct is % of bankroll and must stay ≤ 5 unless user policy says otherwise. Never output fractional_kelly near 100% for long-shots.
- RESOLUTION FIRST: quote the injected settlement rules. For multi-candidate / jungle primaries, only the named candidate resolves YES; mutual exclusivity with siblings applies — do not treat a party-level narrative as the contract.
- JSON must be valid. No trailing commas. Place it FIRST in the response.

After the JSON, provide a concise readable summary:
- DECISION: [TAKE/WATCH/PASS] [YES/NO] at [price in dollars 0–1]
- PRICE VS FAIR: [market]% vs [fair]%
- EDGE: [edge points] pts, [EV ROI]% EV ROI
- SIZE: [raw Kelly]% raw Kelly, [fractional Kelly]% recommended (capped)
- WHY: [thesis]
- RISK CONTROL: [key risk flags and invalidation conditions]
- RULES CHECK: [one line: what must happen for YES, and any mutual-exclusivity note]
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
        // fractional Kelly is quarter-Kelly then hard-capped at max_bet 5% of bankroll
        assert!((decision.fractional_kelly_pct - 5.0).abs() < 0.05);
    }

    #[test]
    fn placeholder_ticker_detection() {
        assert!(KalshiTradeDecision::is_placeholder_ticker("KXEVENT-TICKER"));
        assert!(KalshiTradeDecision::is_placeholder_ticker("TICKER"));
        assert!(KalshiTradeDecision::is_placeholder_ticker(""));
        assert!(!KalshiTradeDecision::is_placeholder_ticker(
            "KXNBASUMMERSPREAD-26JUL13MINPOR-POR16"
        ));
        assert!(!KalshiTradeDecision::is_placeholder_ticker("KXTEST-1"));
    }

    #[test]
    fn quality_rail_forces_pass_when_spread_exceeds_edge() {
        let mut d = KalshiTradeDecision::new("KXTEST-RAIL-1", "Wide book");
        d.contract_side = ContractSide::YES;
        d.market_price_pct = 50.0;
        d.fair_probability_pct = 55.0;
        d.edge_points = 5.0;
        d.spread_cents = 12.0;
        d.decision = DecisionAction::TAKE;
        d.confidence_tier = ConfidenceTier::Medium;
        d.recommended_stake_dollars = 40.0;
        d.data_quality = DataQuality::Live;
        d.enforce_prediction_quality_rails();
        assert_eq!(d.decision, DecisionAction::PASS);
        assert!(d.risk_flags.contains(&RiskFlag::SpreadExceedsEdge));
        assert_eq!(d.recommended_stake_dollars, 0.0);
    }

    #[test]
    fn quality_rail_demotes_longshot_multiply_without_live_data() {
        let mut d = KalshiTradeDecision::new("KXPRESNOMD-28-ESLO", "Longshot");
        d.contract_side = ContractSide::YES;
        d.market_price_pct = 0.45;
        d.fair_probability_pct = 40.0; // ~89x market — absurd without hard evidence
        d.edge_points = 39.55;
        d.spread_cents = 1.0;
        d.decision = DecisionAction::TAKE;
        d.confidence_tier = ConfidenceTier::High;
        d.data_quality = DataQuality::Inferential;
        d.recommended_stake_dollars = 25.0;
        d.enforce_prediction_quality_rails();
        assert_eq!(d.decision, DecisionAction::WATCH);
        assert_eq!(d.confidence_tier, ConfidenceTier::Low);
        assert!(d.risk_flags.contains(&RiskFlag::ExtremeProbability));
    }

    #[test]
    fn quality_rail_rejects_placeholder_ticker_as_pass() {
        let mut d = KalshiTradeDecision::new("KXEVENT-TICKER", "Schema example");
        d.decision = DecisionAction::TAKE;
        d.recommended_stake_dollars = 50.0;
        d.enforce_prediction_quality_rails();
        assert_eq!(d.decision, DecisionAction::PASS);
        assert_eq!(d.recommended_stake_dollars, 0.0);
    }

    #[test]
    fn quality_rail_does_not_change_fair_probability() {
        // Math/fair value must be left alone — rails only affect action/confidence.
        let mut d = KalshiTradeDecision::new("KXTEST-RAIL-2", "Preserve fair");
        d.fair_probability_pct = 12.5;
        d.market_price_pct = 1.0;
        d.edge_points = 11.5;
        d.spread_cents = 1.0;
        d.decision = DecisionAction::TAKE;
        d.data_quality = DataQuality::Inferential;
        d.enforce_prediction_quality_rails();
        assert!((d.fair_probability_pct - 12.5).abs() < 1e-12);
        assert!((d.market_price_pct - 1.0).abs() < 1e-12);
    }

    #[test]
    fn coerce_price_accepts_dollars_and_cents() {
        assert!((coerce_price_to_dollars(0.55) - 0.55).abs() < 1e-9);
        assert!((coerce_price_to_dollars(55.0) - 0.55).abs() < 1e-9);
        assert!((coerce_price_to_dollars(0.0) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn coerce_prob_accepts_unit_interval_and_percent() {
        assert!((coerce_probability_to_pct(0.62) - 62.0).abs() < 1e-9);
        assert!((coerce_probability_to_pct(62.0) - 62.0).abs() < 1e-9);
    }

    #[test]
    fn sanitize_caps_absurd_fractional_kelly_before_take() {
        let mut decision = KalshiTradeDecision::new("KX-HILTON", "Jungle primary candidate");
        decision.contract_side = ContractSide::YES;
        decision.decision = DecisionAction::TAKE;
        // LLM mixed units: market as 0.1 (dollars → 10¢) and claimed 99.8% fractional Kelly
        decision.market_price_pct = 0.1;
        decision.fair_probability_pct = 40.0;
        decision.price_to_enter = 0.1;
        decision.raw_kelly_pct = 99.8;
        decision.fractional_kelly_pct = 99.8;
        decision.recommended_stake_dollars = 9980.0;
        decision.sanitize_units_and_caps(10_000.0, 0.25, 0.05);

        assert!((decision.market_price_pct - 10.0).abs() < 0.01);
        assert!((decision.price_to_enter - 0.1).abs() < 1e-9);
        assert!(decision.fractional_kelly_pct <= 5.0 + 1e-9);
        assert!(decision.recommended_stake_dollars <= 500.0 + 1e-6);
        assert!(decision.risk_flags.contains(&RiskFlag::BankrollLimitExceeded));
    }

    #[test]
    fn sanitize_normalizes_price_to_enter_from_cents() {
        let mut decision = KalshiTradeDecision::new("KX-TEST", "Test");
        decision.market_price_pct = 55.0;
        decision.fair_probability_pct = 60.0;
        decision.price_to_enter = 55.0; // cents by mistake
        decision.sanitize_units_and_caps(1000.0, 0.25, 0.05);
        assert!((decision.price_to_enter - 0.55).abs() < 1e-9);
        assert!((decision.market_price_pct - 55.0).abs() < 1e-9);
    }

    #[test]
    fn coerce_preserves_sub_one_percent_with_dollar_entry() {
        // Slotkin-style: market 0.45% written as 0.45, entry $0.005
        let (pct, enter) = coerce_market_and_entry(0.45, 0.005);
        assert!((pct - 0.45).abs() < 1e-9, "pct={pct}");
        assert!((enter - 0.005).abs() < 1e-9);
    }

    #[test]
    fn coerce_aligns_matching_dollar_market_and_entry() {
        let (pct, enter) = coerce_market_and_entry(0.55, 0.55);
        assert!((pct - 55.0).abs() < 1e-9);
        assert!((enter - 0.55).abs() < 1e-9);
    }

    #[test]
    fn enforce_settlement_gate_kills_take() {
        let mut d = KalshiTradeDecision::new("KX-DONE", "Done");
        d.decision = DecisionAction::TAKE;
        d.contract_side = ContractSide::YES;
        d.recommended_stake_dollars = 100.0;
        d.fractional_kelly_pct = 5.0;
        let gate = crate::chat::market_gate::MarketGate::Settled {
            reason: "result=Yes".into(),
            result: Some("Yes".into()),
        };
        d.enforce_settlement_gate(&gate);
        assert_eq!(d.decision, DecisionAction::PASS);
        assert_eq!(d.contract_side, ContractSide::PASS);
        assert_eq!(d.recommended_stake_dollars, 0.0);
        assert!(d.risk_flags.contains(&RiskFlag::MarketSettledOrClosed));
        assert!(d.thesis.contains("settlement rail"));
    }
}
