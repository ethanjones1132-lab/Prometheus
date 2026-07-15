//! Robust extraction of `KalshiTradeDecision` from LLM text.
//!
//! LLM outputs are noisy: markdown fences, trailing commas, inline comments,
//! mixed price/probability units, and partial JSON. This module centralizes
//! the extraction logic so callers get either a validated decision or a
//! descriptive error instead of silent `.ok()` failures.

use crate::chat::decision_schema::KalshiTradeDecision;
use serde_json::Value;
use std::fmt;

/// Why a decision could not be extracted from the provided text.
#[derive(Debug, Clone, PartialEq)]
pub enum DecisionExtractError {
    NoJsonObject,
    InvalidJson(String),
    MissingTicker,
    NotAKalshiDecision,
    Deserialize(String),
}

impl fmt::Display for DecisionExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecisionExtractError::NoJsonObject => write!(f, "no JSON object found in text"),
            DecisionExtractError::InvalidJson(e) => write!(f, "invalid JSON: {}", e),
            DecisionExtractError::MissingTicker => write!(f, "decision missing ticker"),
            DecisionExtractError::NotAKalshiDecision => write!(f, "JSON object does not look like a Kalshi trade decision"),
            DecisionExtractError::Deserialize(e) => write!(f, "decision schema mismatch: {}", e),
        }
    }
}

impl std::error::Error for DecisionExtractError {}

/// Extract every JSON-ish candidate from `text`.
///
/// Returns the contents of ````json ... ```` fences first, then any line that
/// starts with `{` and ends with `}` as a fallback. The strings are *not*
/// repaired yet; run [`repair_json`] before parsing.
pub fn extract_json_candidates(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut search_start = 0;

    // Markdown JSON fences.
    while let Some(block_start) = text[search_start..].find("```json") {
        let abs_start = search_start + block_start + 7;
        if let Some(block_end) = text[abs_start..].find("```") {
            let candidate = text[abs_start..abs_start + block_end].trim().to_string();
            if !candidate.is_empty() {
                candidates.push(candidate);
            }
            search_start = abs_start + block_end + 3;
        } else {
            break;
        }
    }

    // Also accept plain ``` fences that contain JSON (some models omit "json").
    search_start = 0;
    while let Some(block_start) = text[search_start..].find("```") {
        // Skip the opening fence itself.
        let abs_start = search_start + block_start + 3;
        if let Some(block_end) = text[abs_start..].find("```") {
            let candidate = text[abs_start..abs_start + block_end].trim().to_string();
            // Only keep it if it looks like a JSON object and we haven't already captured it.
            if candidate.starts_with('{')
                && candidate.ends_with('}')
                && !candidates.iter().any(|c| c == &candidate)
            {
                candidates.push(candidate);
            }
            search_start = abs_start + block_end + 3;
        } else {
            break;
        }
    }

    // Inline JSON objects, one per line.
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            let candidate = trimmed.to_string();
            if !candidates.iter().any(|c| c == &candidate) {
                candidates.push(candidate);
            }
        }
    }

    candidates
}

/// Apply low-risk repairs that LLMs commonly get wrong.
///
/// - Remove `// ...` line comments.
/// - Remove `/* ... */` block comments.
/// - Remove trailing commas before `]` or `}`.
/// - Strip a leading BOM and surrounding backticks.
pub fn repair_json(raw: &str) -> String {
    let mut s = raw.trim().to_string();

    // Strip Unicode BOM.
    if s.starts_with('\u{feff}') {
        s = s[3..].to_string();
    }

    // Remove // line comments (avoid URLs inside strings — crude but safe for
    // decision JSON because URLs are not expected).
    let mut out = String::with_capacity(s.len());
    let mut in_string = false;
    let mut escape = false;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            out.push(b as char);
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        // Block comment start.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let end = s[i + 2..].find("*/").map(|j| i + 2 + j + 2).unwrap_or(bytes.len());
            i = end;
            continue;
        }

        // Line comment start.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        if b == b'"' {
            in_string = true;
        }
        out.push(b as char);
        i += 1;
    }

    // Remove trailing commas: `,]` and `,}` outside strings, allowing whitespace.
    let mut repaired = String::with_capacity(out.len());
    let bytes = out.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b',' {
            // Peek ahead, skipping whitespace, to see if this comma precedes ] or }.
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b']' || bytes[j] == b'}') {
                // Drop the comma but keep the whitespace + bracket.
                i += 1;
                continue;
            }
        }
        repaired.push(bytes[i] as char);
        i += 1;
    }

    repaired
}

/// Heuristic: does this JSON value contain the minimum fields of a Kalshi trade decision?
pub fn looks_like_kalshi_decision(val: &Value) -> bool {
    val.get("ticker")
        .and_then(|v| v.as_str())
        .map(|t| !t.is_empty() && (t.starts_with("KX") || t.contains('-')))
        .unwrap_or(false)
        && val.get("fair_probability_pct").is_some()
}

/// Parse a repaired JSON string into a validated `KalshiTradeDecision`.
///
/// On success, units are sanitized via [`KalshiTradeDecision::sanitize_units_and_caps`]
/// using conservative defaults (bankroll = $10_000, kelly_fraction = 0.25, max_bet_pct = 0.05).
pub fn parse_kalshi_trade_decision(json: &str) -> Result<KalshiTradeDecision, DecisionExtractError> {
    let repaired = repair_json(json);
    let val: Value = serde_json::from_str(&repaired)
        .map_err(|e| DecisionExtractError::InvalidJson(e.to_string()))?;

    if !val.is_object() {
        return Err(DecisionExtractError::NoJsonObject);
    }

    if !looks_like_kalshi_decision(&val) {
        return Err(DecisionExtractError::NotAKalshiDecision);
    }

    let mut decision: KalshiTradeDecision = serde_json::from_value(val)
        .map_err(|e| DecisionExtractError::Deserialize(e.to_string()))?;

    if decision.ticker.trim().is_empty() {
        return Err(DecisionExtractError::MissingTicker);
    }

    // Normalize units and cap sizing before any downstream use. Use conservative
    // defaults; the UI/command path can re-sanitize with the user's actual bankroll.
    decision.sanitize_units_and_caps(10_000.0, 0.25, 0.05);
    // Quality rails (no math changes): placeholder tickers, spread>edge, longshot multiplies.
    decision.enforce_prediction_quality_rails();
    if KalshiTradeDecision::is_placeholder_ticker(&decision.ticker) {
        return Err(DecisionExtractError::NotAKalshiDecision);
    }
    Ok(decision)
}

/// Convenience: scan `text` for the best valid Kalshi trade decision.
///
/// Free models often emit several JSON revisions after a monologue. Prefer the
/// **last** valid non-placeholder decision (refined ticket) rather than the first
/// scratch copy that may still use schema examples.
pub fn find_kalshi_decision_in_text(text: &str) -> Result<KalshiTradeDecision, DecisionExtractError> {
    let mut last_ok: Option<KalshiTradeDecision> = None;
    let mut last_err = DecisionExtractError::NoJsonObject;
    for candidate in extract_json_candidates(text) {
        match parse_kalshi_trade_decision(&candidate) {
            Ok(d) => {
                last_ok = Some(d);
            }
            Err(e) => {
                last_err = e.clone();
                tracing::debug!(
                    "kalshi decision candidate failed: {} | json: {}",
                    e,
                    candidate.chars().take(200).collect::<String>()
                );
            }
        }
    }
    last_ok.ok_or(last_err)
}

/// Parse a decision that was stored as a plain JSON blob (e.g. from the database).
/// Repairs are applied just like for fresh LLM output.
pub fn parse_kalshi_decision_blob(blob: &str) -> Result<KalshiTradeDecision, DecisionExtractError> {
    parse_kalshi_trade_decision(blob)
}

/// Parse from an already-deserialized `serde_json::Value`.
/// Handy when the caller has already parsed the surrounding JSON envelope.
pub fn parse_kalshi_trade_decision_from_value(val: &Value) -> Result<KalshiTradeDecision, DecisionExtractError> {
    if !val.is_object() {
        return Err(DecisionExtractError::NoJsonObject);
    }
    if !looks_like_kalshi_decision(val) {
        return Err(DecisionExtractError::NotAKalshiDecision);
    }
    let mut decision: KalshiTradeDecision = serde_json::from_value(val.clone())
        .map_err(|e| DecisionExtractError::Deserialize(e.to_string()))?;
    if decision.ticker.trim().is_empty() {
        return Err(DecisionExtractError::MissingTicker);
    }
    decision.sanitize_units_and_caps(10_000.0, 0.25, 0.05);
    decision.enforce_prediction_quality_rails();
    if KalshiTradeDecision::is_placeholder_ticker(&decision.ticker) {
        return Err(DecisionExtractError::NotAKalshiDecision);
    }
    Ok(decision)
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::decision_schema::{ContractSide, DecisionAction};

    fn minimal_valid() -> String {
        r#"{
            "ticker": "KXTEST-1",
            "market_title": "Test",
            "category": "Economics",
            "contract_side": "YES",
            "market_price_pct": 55.0,
            "fair_probability_pct": 60.0,
            "edge_points": 5.0,
            "spread_cents": 2.0,
            "liquidity_score": 80.0,
            "ev_per_contract_cents": 5.0,
            "ev_roi_pct": 9.0,
            "raw_kelly_pct": 11.0,
            "fractional_kelly_pct": 2.8,
            "recommended_stake_dollars": 280.0,
            "max_position_dollars": 280.0,
            "decision": "TAKE",
            "confidence_tier": "Medium",
            "thesis": "Edge.",
            "evidence": ["A"],
            "risk_flags": [],
            "data_quality": "Live",
            "price_to_enter": 0.55,
            "model_disagreement": false
        }"#.to_string()
    }

    #[test]
    fn parses_valid_markdown_block() {
        let text = format!("Some summary.\n\n```json\n{}\n```\nMore text.", minimal_valid());
        let d = find_kalshi_decision_in_text(&text).unwrap();
        assert_eq!(d.ticker, "KXTEST-1");
        assert_eq!(d.contract_side, ContractSide::YES);
        assert_eq!(d.decision, DecisionAction::TAKE);
    }

    #[test]
    fn repairs_trailing_comma_and_comment() {
        let json = r#"{
            "ticker": "KXTEST-2",
            "market_title": "Test",
            "category": "Economics",
            "contract_side": "YES",
            "market_price_pct": 55.0,
            "fair_probability_pct": 60.0,
            "edge_points": 5.0,
            "spread_cents": 2.0,
            "liquidity_score": 80.0,
            "ev_per_contract_cents": 5.0,
            "ev_roi_pct": 9.0,
            "raw_kelly_pct": 11.0,
            "fractional_kelly_pct": 2.8,
            "recommended_stake_dollars": 280.0,
            "max_position_dollars": 280.0,
            "decision": "TAKE",
            "confidence_tier": "Medium",
            "thesis": "Edge.",
            "evidence": ["A",], // trailing comma
            "risk_flags": [],
            "data_quality": "Live",
            "price_to_enter": 0.55,
            "model_disagreement": false,
        }"#;
        let d = parse_kalshi_trade_decision(json).unwrap();
        assert_eq!(d.ticker, "KXTEST-2");
    }

    #[test]
    fn rejects_non_kalshi_json() {
        let json = r#"{"player": "Mahomes", "pick": "Over", "line": 245.5}"#;
        let err = parse_kalshi_trade_decision(json).unwrap_err();
        assert!(matches!(err, DecisionExtractError::NotAKalshiDecision));
    }

    #[test]
    fn sanitizes_cents_to_dollars() {
        let json = r#"{
            "ticker": "KXTEST-3",
            "market_title": "Test",
            "category": "Economics",
            "contract_side": "YES",
            "market_price_pct": 55.0,
            "fair_probability_pct": 0.60,
            "edge_points": 5.0,
            "spread_cents": 2.0,
            "liquidity_score": 80.0,
            "ev_per_contract_cents": 5.0,
            "ev_roi_pct": 9.0,
            "raw_kelly_pct": 11.0,
            "fractional_kelly_pct": 2.8,
            "recommended_stake_dollars": 280.0,
            "max_position_dollars": 280.0,
            "decision": "TAKE",
            "confidence_tier": "Medium",
            "thesis": "Edge.",
            "evidence": ["A"],
            "risk_flags": [],
            "data_quality": "Live",
            "price_to_enter": 55.0,
            "model_disagreement": false
        }"#;
        let d = parse_kalshi_trade_decision(json).unwrap();
        assert!((d.price_to_enter - 0.55).abs() < 1e-9);
        assert!((d.fair_probability_pct - 60.0).abs() < 1e-9);
    }

    #[test]
    fn caps_absurd_fractional_kelly() {
        let mut json = minimal_valid();
        json = json.replace("\"fractional_kelly_pct\": 2.8", "\"fractional_kelly_pct\": 99.8");
        let d = parse_kalshi_trade_decision(&json).unwrap();
        assert!(d.fractional_kelly_pct <= 5.0 + 1e-9);
    }

    #[test]
    fn rejects_placeholder_schema_ticker() {
        let mut json = minimal_valid();
        json = json.replace("KXTEST-1", "KXEVENT-TICKER");
        let err = parse_kalshi_trade_decision(&json).unwrap_err();
        assert!(matches!(err, DecisionExtractError::NotAKalshiDecision));
    }

    #[test]
    fn prefers_last_valid_decision_block() {
        let first = r#"```json
{
  "ticker": "KXEVENT-TICKER",
  "market_title": "placeholder",
  "category": "Other",
  "contract_side": "YES",
  "market_price_pct": 50.0,
  "fair_probability_pct": 60.0,
  "edge_points": 10.0,
  "spread_cents": 1.0,
  "liquidity_score": 80.0,
  "ev_per_contract_cents": 10.0,
  "ev_roi_pct": 20.0,
  "raw_kelly_pct": 20.0,
  "fractional_kelly_pct": 5.0,
  "recommended_stake_dollars": 50.0,
  "max_position_dollars": 50.0,
  "decision": "TAKE",
  "confidence_tier": "High",
  "thesis": "bad",
  "evidence": [],
  "risk_flags": [],
  "data_quality": "Live",
  "price_to_enter": 0.5,
  "model_disagreement": false
}
```"#;
        let second = minimal_valid();
        let text = format!("{first}\n\n```json\n{second}\n```");
        let d = find_kalshi_decision_in_text(&text).unwrap();
        assert_eq!(d.ticker, "KXTEST-1");
    }
}
