//! Kalshi ticker structure — the one place that knows how to read a market
//! ticker string.
//!
//! Two facts are recoverable from a ticker and both matter for honest
//! measurement of the forecast ledger:
//!
//! 1. **Which underlying event it belongs to.** A ticker's final `-SEGMENT` is
//!    its strike / outcome leg (`-MIA10`, `-TOR8`, `-POS`). Ten legs of one
//!    baseball game are ten rows in the ledger but **one** observation of the
//!    world — treating them as ten independent samples inflates any sample
//!    count by an order of magnitude. [`event_key`] strips that leg so
//!    correlated rows group together.
//!
//! 2. **When the underlying event started.** Sports/esports/crypto tickers
//!    embed `YYMMMDDHHMM` (e.g. `26JUL191215` = 2026-07-19 12:15). A forecast
//!    written *after* tip-off is not a forecast — the price it copies already
//!    contains the score. [`event_start_from_ticker`] recovers that instant so
//!    such rows can be excluded from skill measurement.
//!
//! Both functions are pure and total: no I/O, no panics, `None`/identity on
//! anything they cannot read. **When a ticker does not encode a time, that is
//! reported as `None` — never guessed.** A wrong start time silently
//! reclassifies in-play rows as pre-event, which is exactly the failure this
//! module exists to prevent.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

/// Kalshi embeds event times in **US Eastern**, not UTC. The ledger this
/// module serves is entirely July 2026 data, where Eastern is EDT = UTC−4, so
/// UTC = embedded + 4h.
///
/// This is a fixed offset, not a timezone database lookup: during EST
/// (November–March) the true offset is 5h and this parser will be one hour
/// early. One hour early makes `is_in_play` *more* conservative for the
/// pre-event side (an event looks like it started later than it did, so
/// borderline rows are more likely to be called pre-event) — so if this is
/// ever run on winter data, prefer wiring a real `chrono-tz`
/// `America/New_York` lookup over widening the fudge.
const ET_TO_UTC_OFFSET_HOURS: i64 = 4;

const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// The ticker with its final `-SEGMENT` (the strike / outcome leg) removed.
///
/// Correlated legs of one underlying event collapse to the same key:
///
/// ```text
/// KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR8  →  KXMLBTEAMTOTAL-26JUL191215CWSTOR
/// KXMLBTEAMTOTAL-26JUL191215CWSTOR-CWS3  →  KXMLBTEAMTOTAL-26JUL191215CWSTOR
/// SENATETX-26-R                          →  SENATETX-26
/// ```
///
/// A ticker with no hyphen has no leg to strip and is returned unchanged.
pub fn event_key(ticker: &str) -> String {
    match ticker.rsplit_once('-') {
        Some((head, _)) if !head.is_empty() => head.to_string(),
        _ => ticker.to_string(),
    }
}

/// The UTC start instant of the underlying event, when the ticker encodes one.
///
/// Recognised form is a segment beginning `YYMMMDDHHMM` — two-digit year,
/// uppercase three-letter month, two-digit day, then a four-digit Eastern
/// `HHMM` — optionally followed by non-digit text (team codes):
///
/// ```text
/// KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR8  →  2026-07-19 16:15Z  (12:15 ET)
/// KXMLBTEAMTOTAL-26JUL181510CINCOL-CIN4  →  2026-07-18 19:10Z  (15:10 ET)
/// KXMLBTOTAL-26JUL171915CWSTOR-4         →  2026-07-17 23:15Z  (19:15 ET)
/// ```
///
/// Returns `None` when no time is encoded — a date-only segment
/// (`-26JUL11DENMIN`), a truncated hour (`KXBTC-26JUL1813`, two digits, not
/// four), or no date at all (`KXWORLDCUPHALFTIME-26`). Guessing midnight, or
/// reading `13` as `13:00`, would fabricate an ordering between the forecast
/// and the event that the ticker does not actually assert.
pub fn event_start_from_ticker(ticker: &str) -> Option<DateTime<Utc>> {
    ticker.split('-').find_map(parse_datetime_prefix)
}

/// Parse a leading `YYMMMDDHHMM` out of one ticker segment.
fn parse_datetime_prefix(segment: &str) -> Option<DateTime<Utc>> {
    let b = segment.as_bytes();
    if !segment.is_ascii() || b.len() < 11 {
        return None;
    }

    let year = two_digits(&b[0..2])? as i32 + 2000;
    let month = month_number(&segment[2..5])?;
    let day = two_digits(&b[5..7])?;
    let hour = two_digits(&b[7..9])?;
    let minute = two_digits(&b[9..11])?;

    // The digit run must stop here. If a 12th digit follows, these eleven
    // characters are a prefix of something longer and reading them as a
    // timestamp would be a coincidence, not a parse.
    if b.get(11).is_some_and(u8::is_ascii_digit) {
        return None;
    }

    if hour > 23 || minute > 59 {
        return None;
    }

    let naive = NaiveDate::from_ymd_opt(year, month, day as u32)?
        .and_hms_opt(hour as u32, minute as u32, 0)?;
    let eastern_as_utc = Utc.from_utc_datetime(&naive);
    eastern_as_utc.checked_add_signed(chrono::Duration::hours(ET_TO_UTC_OFFSET_HOURS))
}

fn two_digits(bytes: &[u8]) -> Option<u8> {
    if bytes.len() != 2 || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some((bytes[0] - b'0') * 10 + (bytes[1] - b'0'))
}

fn month_number(name: &str) -> Option<u32> {
    MONTHS
        .iter()
        .position(|m| *m == name)
        .map(|i| i as u32 + 1)
}

/// `true` when `created_at` is at or after the event start — i.e. the quote
/// this forecast copied already contained the outcome in progress.
///
/// `false` when the ticker encodes no start time: absence of evidence is not
/// evidence of in-play, and the ledger records the unknown as a NULL
/// `event_start_at` alongside it.
pub fn is_in_play(created_at: &str, event_start: Option<DateTime<Utc>>) -> bool {
    match (parse_timestamp(created_at), event_start) {
        (Some(created), Some(start)) => created >= start,
        _ => false,
    }
}

/// Parse a ledger timestamp. Rows have been written by three different code
/// paths over time (`to_rfc3339`, Python `isoformat`, and a millisecond `Z`
/// format), so accept RFC 3339 first and fall back to a naive parse for the
/// zone-less variant.
pub fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(Utc.from_utc_datetime(&naive));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.from_utc_datetime(&NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap())
    }

    // ---- event_key ----

    #[test]
    fn event_key_strips_the_final_strike_segment() {
        // Every vector below is a real ticker from the live ledger.
        assert_eq!(
            event_key("KXNBASUMMERSPREAD-26JUL11MIAORL-MIA10"),
            "KXNBASUMMERSPREAD-26JUL11MIAORL"
        );
        assert_eq!(
            event_key("KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR8"),
            "KXMLBTEAMTOTAL-26JUL191215CWSTOR"
        );
        assert_eq!(event_key("KXWORLDCUPHALFTIME-26-POS"), "KXWORLDCUPHALFTIME-26");
        assert_eq!(event_key("SENATETX-26-R"), "SENATETX-26");
    }

    #[test]
    fn event_key_groups_correlated_legs_of_one_game() {
        let legs = [
            "KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR2",
            "KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR8",
            "KXMLBTEAMTOTAL-26JUL191215CWSTOR-CWS3",
        ];
        let keys: std::collections::HashSet<String> = legs.iter().map(|t| event_key(t)).collect();
        assert_eq!(keys.len(), 1, "one game must yield one key, got {keys:?}");
    }

    #[test]
    fn event_key_of_hyphenless_ticker_is_itself() {
        assert_eq!(event_key("KXTEST"), "KXTEST");
        assert_eq!(event_key(""), "");
    }

    #[test]
    fn event_key_keeps_the_map_number_on_multi_leg_esports() {
        // Map 1 and map 2 of the same series are genuinely different events;
        // only the team leg is stripped.
        assert_eq!(
            event_key("KXVALORANTMAP-26JUL101900SRNRGA-1-NRGA"),
            "KXVALORANTMAP-26JUL101900SRNRGA-1"
        );
        assert_ne!(
            event_key("KXVALORANTMAP-26JUL101900SRNRGA-1-NRGA"),
            event_key("KXVALORANTMAP-26JUL101900SRNRGA-2-SR")
        );
    }

    // ---- event_start_from_ticker ----

    /// The segment decomposes `YY MMM DD HHMM`, **not** `DD MMM YY HHMM`.
    /// These three vectors pin it: all are real tickers whose games are known
    /// to have started on the 19th, 18th and 17th of July 2026 respectively.
    #[test]
    fn start_time_decomposes_as_year_month_day_time() {
        assert_eq!(
            event_start_from_ticker("KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR8"),
            Some(utc(2026, 7, 19, 16, 15)), // 12:15 ET
        );
        assert_eq!(
            event_start_from_ticker("KXMLBTEAMTOTAL-26JUL181510CINCOL-CIN4"),
            Some(utc(2026, 7, 18, 19, 10)), // 15:10 ET
        );
        assert_eq!(
            event_start_from_ticker("KXMLBTOTAL-26JUL171915CWSTOR-4"),
            Some(utc(2026, 7, 17, 23, 15)), // 19:15 ET
        );
    }

    #[test]
    fn start_time_is_eastern_converted_to_utc() {
        // 20:08 ET on 2026-07-18 is 00:08 UTC the *next* day.
        assert_eq!(
            event_start_from_ticker("KXMLBGAME-26JUL182008LADNYY-LAD"),
            Some(utc(2026, 7, 19, 0, 8)),
        );
    }

    #[test]
    fn date_only_ticker_yields_no_start_time() {
        // `26JUL11` encodes a day but no time — midnight would be a guess.
        assert_eq!(event_start_from_ticker("KXNBASUMMERTOTAL-26JUL11DENMIN-184"), None);
        assert_eq!(event_start_from_ticker("KXHIGHCHI-26JUL18-B89.5"), None);
    }

    #[test]
    fn truncated_two_digit_hour_yields_no_start_time() {
        // `26JUL1813` has two trailing digits, not four. Reading them as
        // "13:00" would be inference, not parsing.
        assert_eq!(event_start_from_ticker("KXBTC-26JUL1813-B64050"), None);
        assert_eq!(event_start_from_ticker("KXGOLDH-26JUL1612-T3979.99"), None);
    }

    #[test]
    fn ticker_without_a_date_yields_no_start_time() {
        assert_eq!(event_start_from_ticker("KXWORLDCUPHALFTIME-26-POS"), None);
        assert_eq!(event_start_from_ticker("SENATETX-26-R"), None);
        assert_eq!(event_start_from_ticker("KXTEST"), None);
        assert_eq!(event_start_from_ticker(""), None);
    }

    #[test]
    fn a_longer_digit_run_is_not_mistaken_for_a_timestamp() {
        // Twelve consecutive digits after the month: the eleven-char prefix
        // must not be harvested as a time.
        assert_eq!(event_start_from_ticker("KXFOO-26JUL1912150-X"), None);
    }

    #[test]
    fn impossible_clock_and_calendar_values_are_rejected() {
        assert_eq!(event_start_from_ticker("KXFOO-26JUL1999159-X"), None); // trailing digit
        assert_eq!(event_start_from_ticker("KXFOO-26JUL192599ABC"), None); // 25:99
        assert_eq!(event_start_from_ticker("KXFOO-26FEB301200ABC"), None); // Feb 30
        assert_eq!(event_start_from_ticker("KXFOO-26XXX191215ABC"), None); // no such month
    }

    #[test]
    fn lowercase_month_is_not_accepted() {
        // Kalshi tickers are uppercase; a lowercase match would mean the
        // caller handed us something other than a ticker.
        assert_eq!(event_start_from_ticker("KXFOO-26jul191215ABC"), None);
    }

    // ---- is_in_play ----

    #[test]
    fn in_play_when_created_after_first_pitch() {
        let start = event_start_from_ticker("KXMLBTOTAL-26JUL171915CWSTOR-4");
        assert!(start.is_some());
        assert!(is_in_play("2026-07-18T02:30:00Z", start), "logged mid-game");
        assert!(!is_in_play("2026-07-17T18:00:00Z", start), "logged pre-game");
        // Exactly at the start counts as in-play: the tape is already live.
        assert!(is_in_play("2026-07-17T23:15:00Z", start));
    }

    #[test]
    fn unknown_start_is_never_reported_as_in_play() {
        assert!(!is_in_play("2026-07-18T02:30:00Z", None));
        assert!(!is_in_play(
            "2026-07-18T02:30:00Z",
            event_start_from_ticker("KXWORLDCUPHALFTIME-26-POS")
        ));
    }

    #[test]
    fn unparseable_created_at_is_never_reported_as_in_play() {
        let start = event_start_from_ticker("KXMLBTOTAL-26JUL171915CWSTOR-4");
        assert!(!is_in_play("not a timestamp", start));
    }

    #[test]
    fn timestamp_parser_accepts_every_format_the_ledger_contains() {
        // chrono rfc3339 (Rust `to_rfc3339`)
        assert!(parse_timestamp("2026-07-19T02:19:21.571103500+00:00").is_some());
        // Python isoformat with Z, millisecond precision
        assert!(parse_timestamp("2026-07-19T02:19:21.571Z").is_some());
        // zone-less naive (treated as UTC)
        assert_eq!(
            parse_timestamp("2026-07-19T02:19:21"),
            Some(utc(2026, 7, 19, 2, 19).checked_add_signed(chrono::Duration::seconds(21)).unwrap())
        );
        assert!(parse_timestamp("").is_none());
    }
}
