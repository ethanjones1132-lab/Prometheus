//! Deterministic settlement / tradability gates from Kalshi tape metadata.
//!
//! Tape truth beats narrative. A market that already has a result, is settled,
//! or is past its close time must never be framed as an open "mispriced" TAKE.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Tradability assessment for a single market.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MarketGate {
    /// Open for analysis / optional TAKE.
    Open,
    /// Event appears finished or market not tradeable — force PASS.
    Settled {
        reason: String,
        result: Option<String>,
    },
    /// Closed to new orders but may not have a final result yet.
    Closed { reason: String },
}

impl MarketGate {
    pub fn allows_take(&self) -> bool {
        matches!(self, MarketGate::Open)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, MarketGate::Settled { .. } | MarketGate::Closed { .. })
    }

    pub fn label(&self) -> &'static str {
        match self {
            MarketGate::Open => "OPEN",
            MarketGate::Settled { .. } => "SETTLED",
            MarketGate::Closed { .. } => "CLOSED",
        }
    }
}

/// Assess tradability from Kalshi fields (no network).
pub fn assess_market_gate(
    status: &str,
    result: &str,
    close_time: Option<&str>,
    expiration_time: Option<&str>,
    now: DateTime<Utc>,
) -> MarketGate {
    assess_market_gate_for_ticker(None, status, result, close_time, expiration_time, now)
}

/// Like [`assess_market_gate`] but also parses embedded event dates from tickers
/// (e.g. `KXCAGOVPRIMARY1ST-26JUN02-1ST-XBEC` → 2026-06-02).
pub fn assess_market_gate_for_ticker(
    ticker: Option<&str>,
    status: &str,
    result: &str,
    close_time: Option<&str>,
    expiration_time: Option<&str>,
    now: DateTime<Utc>,
) -> MarketGate {
    let status_l = status.trim().to_lowercase();
    let result_t = result.trim();

    if !result_t.is_empty() {
        return MarketGate::Settled {
            reason: format!("market has settlement result={result_t}"),
            result: Some(result_t.to_string()),
        };
    }

    if matches!(
        status_l.as_str(),
        "settled" | "finalized" | "determined" | "resolved"
    ) {
        return MarketGate::Settled {
            reason: format!("status={status}"),
            result: None,
        };
    }

    if matches!(status_l.as_str(), "closed" | "inactive") {
        return MarketGate::Closed {
            reason: format!("status={status}"),
        };
    }

    // Past close / expiration → treat as non-tradeable (near-settled).
    for (label, raw) in [("close_time", close_time), ("expiration_time", expiration_time)] {
        if let Some(ts) = raw.and_then(parse_kalshi_time) {
            if ts < now {
                return MarketGate::Settled {
                    reason: format!("{label} {ts} is in the past (event window ended)"),
                    result: None,
                };
            }
        }
    }

    // Ticker date heuristic when calendar fields are missing/stale on quick-cache.
    // Example: …-26JUN02-… embeds 2026-06-02 election day.
    if let Some(t) = ticker {
        if let Some((event_day, raw_token)) = parse_embedded_event_date(t) {
            // End of event day UTC — if we're past it, treat as settled.
            let end = event_day
                .and_hms_opt(23, 59, 59)
                .map(|ndt| Utc.from_utc_datetime(&ndt));
            if let Some(end) = end {
                if end < now {
                    return MarketGate::Settled {
                        reason: format!(
                            "ticker embeds event date {raw_token} ({event_day}) which is in the past"
                        ),
                        result: None,
                    };
                }
            }
        }
    }

    MarketGate::Open
}

/// Parse Kalshi-style embedded dates: `26JUN02` → 2026-06-02.
/// Avoids bare year tokens like `-28-` (2028 series) that lack month+day.
pub fn parse_embedded_event_date(ticker: &str) -> Option<(NaiveDate, String)> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?i)(?:^|[^A-Z0-9])(\d{2})(JAN|FEB|MAR|APR|MAY|JUN|JUL|AUG|SEP|OCT|NOV|DEC)(\d{2})(?:$|[^A-Z0-9])",
        )
        .expect("embedded date regex")
    });
    let caps = re.captures(ticker)?;
    let yy: i32 = caps.get(1)?.as_str().parse().ok()?;
    let mon = caps.get(2)?.as_str().to_ascii_uppercase();
    let dd: u32 = caps.get(3)?.as_str().parse().ok()?;
    let month = match mon.as_str() {
        "JAN" => 1,
        "FEB" => 2,
        "MAR" => 3,
        "APR" => 4,
        "MAY" => 5,
        "JUN" => 6,
        "JUL" => 7,
        "AUG" => 8,
        "SEP" => 9,
        "OCT" => 10,
        "NOV" => 11,
        "DEC" => 12,
        _ => return None,
    };
    // Kalshi uses 2-digit year; 00–79 → 2000–2079 (prediction markets horizon).
    let year = 2000 + yy;
    let day = NaiveDate::from_ymd_opt(year, month, dd)?;
    let token = format!("{yy:02}{mon}{dd:02}");
    Some((day, token))
}

/// Parse common Kalshi / RFC3339 timestamps.
pub fn parse_kalshi_time(raw: &str) -> Option<DateTime<Utc>> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    // "2026-06-02T00:00:00Z" variants without offset
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ") {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S %z") {
        return Some(dt.with_timezone(&Utc));
    }
    None
}

/// Format a gate line for prompt injection.
pub fn format_gate_line(ticker: &str, gate: &MarketGate) -> String {
    match gate {
        MarketGate::Open => format!("- [{ticker}] GATE=OPEN — eligible for TAKE/WATCH/PASS"),
        MarketGate::Settled { reason, result } => {
            let res = result
                .as_deref()
                .map(|r| format!(" result={r}"))
                .unwrap_or_default();
            format!(
                "- [{ticker}] GATE=SETTLED{res} — FORCE PASS. Do not invent open-field fair value. ({reason})"
            )
        }
        MarketGate::Closed { reason } => {
            format!(
                "- [{ticker}] GATE=CLOSED — FORCE PASS / no new entry. ({reason})"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn noon_july_10_2026() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 10, 12, 0, 0).unwrap()
    }

    #[test]
    fn settled_when_result_present() {
        let g = assess_market_gate("active", "Yes", None, None, noon_july_10_2026());
        assert!(!g.allows_take());
        assert!(matches!(g, MarketGate::Settled { .. }));
    }

    #[test]
    fn settled_when_status_settled() {
        let g = assess_market_gate("settled", "", None, None, noon_july_10_2026());
        assert!(!g.allows_take());
    }

    #[test]
    fn settled_when_close_in_past() {
        let g = assess_market_gate(
            "active",
            "",
            Some("2026-06-02T23:59:59Z"),
            None,
            noon_july_10_2026(),
        );
        assert!(!g.allows_take());
        match g {
            MarketGate::Settled { reason, .. } => assert!(reason.contains("close_time")),
            _ => panic!("expected Settled"),
        }
    }

    #[test]
    fn open_when_future_close() {
        let g = assess_market_gate(
            "open",
            "",
            Some("2028-11-07T00:00:00Z"),
            None,
            noon_july_10_2026(),
        );
        assert!(g.allows_take());
        assert_eq!(g, MarketGate::Open);
    }

    #[test]
    fn closed_status() {
        let g = assess_market_gate("closed", "", None, None, noon_july_10_2026());
        assert!(!g.allows_take());
        assert!(matches!(g, MarketGate::Closed { .. }));
    }

    #[test]
    fn parse_ticker_26jun02() {
        let (d, tok) = parse_embedded_event_date("KXCAGOVPRIMARY1ST-26JUN02-1ST-XBEC").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 6, 2).unwrap());
        assert!(tok.contains("JUN"));
    }

    #[test]
    fn year_only_28_not_parsed_as_event_day() {
        // KXPRESNOMD-28-ESLO is a 2028 series, not June 28
        assert!(parse_embedded_event_date("KXPRESNOMD-28-ESLO").is_none());
    }

    #[test]
    fn ticker_date_settles_past_primary() {
        let g = assess_market_gate_for_ticker(
            Some("KXCAGOVPRIMARY1ST-26JUN02-1ST-XBEC"),
            "active",
            "",
            None,
            None,
            noon_july_10_2026(),
        );
        assert!(!g.allows_take());
        match g {
            MarketGate::Settled { reason, .. } => assert!(reason.contains("26JUN02") || reason.contains("past")),
            _ => panic!("expected Settled from ticker date"),
        }
    }

    #[test]
    fn future_ticker_date_stays_open() {
        let g = assess_market_gate_for_ticker(
            Some("KXPRESNOMD-28NOV07-SOME"),
            "open",
            "",
            None,
            None,
            noon_july_10_2026(),
        );
        // 2028-11-07 is future
        assert!(g.allows_take());
    }
}
