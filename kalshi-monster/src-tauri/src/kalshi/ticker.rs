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

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, TimeZone, Utc};

/// Kalshi embeds event times in **US Eastern**, not UTC. During EDT (roughly
/// mid-March to early November) Eastern is UTC−4, so UTC = embedded + 4h.
///
/// This is a fixed offset, not a timezone database lookup, so it is only
/// applied inside the EDT window — see [`eastern_is_edt`]. Outside it, the
/// true offset is 5h and this constant would place the event **one hour
/// earlier than it really was**, making a forecast written in that hour look
/// like it came *after* the start when it came before: a row wrongly excluded,
/// or worse, a genuinely in-play row on the other side of the boundary
/// wrongly admitted as evidence. `event_start_from_ticker` returns `None`
/// there instead, which surfaces as an untimed row rather than a silently
/// wrong instant.
///
/// To support winter data, wire `chrono-tz`'s `America/New_York` and delete
/// both this constant and the window check — do not widen the window.
const ET_TO_UTC_OFFSET_HOURS: i64 = 4;

/// `true` when a US Eastern wall-clock instant falls inside daylight saving
/// time: second Sunday of March 02:00 through first Sunday of November 02:00.
///
/// `None` only if the year is outside chrono's representable range.
fn eastern_is_edt(naive: NaiveDateTime) -> Option<bool> {
    let year = naive.year();
    let start = nth_sunday(year, 3, 2)?.and_hms_opt(2, 0, 0)?;
    let end = nth_sunday(year, 11, 1)?.and_hms_opt(2, 0, 0)?;
    Some(naive >= start && naive < end)
}

/// The `n`th Sunday (1-based) of the given month.
fn nth_sunday(year: i32, month: u32, n: u32) -> Option<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let first_sunday = 1 + (7 - first.weekday().num_days_from_sunday()) % 7;
    NaiveDate::from_ymd_opt(year, month, first_sunday + 7 * (n - 1))
}

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
///
/// Also returns `None` for dates outside the EDT window (see
/// [`ET_TO_UTC_OFFSET_HOURS`]): an instant this module knows is an hour wrong
/// is worse than an admitted unknown, because `is_in_play` is computed once at
/// insert and stored.
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
    if !eastern_is_edt(naive)? {
        return None;
    }
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

    // ---- shared vectors ----
    //
    // `scripts/ticker_vectors.json` is the single source of truth for parser
    // behaviour, consumed by this suite and by `scripts/test_kalshi_ticker.py`.
    // Prose cross-references between the two suites went stale immediately last
    // time; a shared file makes drift fail a test instead.

    const VECTORS_JSON: &str = include_str!("../../../../scripts/ticker_vectors.json");

    fn vectors() -> serde_json::Value {
        serde_json::from_str(VECTORS_JSON).expect("scripts/ticker_vectors.json must be valid JSON")
    }

    /// Vector files are only useful if they are actually populated; a silent
    /// empty array would make every vector-driven test vacuously pass.
    #[test]
    fn shared_vector_file_is_populated() {
        let v = vectors();
        assert!(
            v["tickers"].as_array().unwrap().len() >= 25,
            "ticker vectors look truncated"
        );
        assert!(
            v["timestamps"].as_array().unwrap().len() >= 8,
            "timestamp vectors look truncated"
        );
    }

    #[test]
    fn event_key_matches_every_shared_vector() {
        for case in vectors()["tickers"].as_array().unwrap() {
            let ticker = case["ticker"].as_str().unwrap();
            let expected = case["event_key"].as_str().unwrap();
            assert_eq!(
                event_key(ticker),
                expected,
                "event_key({ticker:?}) — {}",
                case["note"].as_str().unwrap_or("")
            );
        }
    }

    #[test]
    fn event_start_matches_every_shared_vector() {
        for case in vectors()["tickers"].as_array().unwrap() {
            let ticker = case["ticker"].as_str().unwrap();
            let expected: Option<DateTime<Utc>> = case["event_start_utc"]
                .as_str()
                .map(|s| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc));
            assert_eq!(
                event_start_from_ticker(ticker),
                expected,
                "event_start_from_ticker({ticker:?}) — {}",
                case["note"].as_str().unwrap_or("")
            );
        }
    }

    #[test]
    fn timestamp_parsing_matches_every_shared_vector() {
        for case in vectors()["timestamps"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            let expected: Option<DateTime<Utc>> = case["parsed_utc"]
                .as_str()
                .map(|s| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc));
            assert_eq!(
                parse_timestamp(input).map(|d| {
                    use chrono::Timelike;
                    d.with_nanosecond(0).unwrap()
                }),
                expected,
                "parse_timestamp({input:?}) — {}",
                case["note"].as_str().unwrap_or("")
            );
        }
    }

    // ---- behaviour the vector file cannot express ----

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
    fn event_key_separates_the_two_maps_of_one_esports_series() {
        // Map 1 and map 2 resolve independently — collapsing them would throw
        // away a real observation.
        assert_ne!(
            event_key("KXVALORANTMAP-26JUL101900SRNRGA-1-NRGA"),
            event_key("KXVALORANTMAP-26JUL101900SRNRGA-2-SR")
        );
    }

    // ---- daylight saving window ----

    /// The +4h constant is EDT-only. Outside the window the true offset is 5h,
    /// so applying +4h would place the event an hour **earlier** than it was —
    /// and because `is_in_play` is computed at insert and stored, a later
    /// timezone fix would not revisit those rows. Refuse instead.
    #[test]
    fn est_dates_are_refused_rather_than_shifted_by_the_wrong_offset() {
        assert_eq!(event_start_from_ticker("KXNFLGAME-26JAN181830KCBUF-KC"), None);
        assert_eq!(event_start_from_ticker("KXNFLGAME-26DEC201300GBCHI-GB"), None);
        // Same wall clock inside the window parses fine, so the refusal above
        // is the window and not a broken parse.
        assert_eq!(
            event_start_from_ticker("KXFOO-26OCT311300AAABBB-X"),
            Some(utc(2026, 10, 31, 17, 0)),
        );
    }

    #[test]
    fn dst_boundaries_are_exact_for_2026() {
        // 2026: DST runs 8 March 02:00 -> 1 November 02:00 (Eastern).
        assert_eq!(eastern_is_edt(naive(2026, 3, 8, 1, 59)), Some(false));
        assert_eq!(eastern_is_edt(naive(2026, 3, 8, 2, 0)), Some(true));
        assert_eq!(eastern_is_edt(naive(2026, 11, 1, 1, 59)), Some(true));
        assert_eq!(eastern_is_edt(naive(2026, 11, 1, 2, 0)), Some(false));
    }

    #[test]
    fn dst_window_tracks_the_calendar_not_a_fixed_date() {
        // 2027: second Sunday of March is the 14th, first Sunday of November
        // the 7th — different dates from 2026, so the rule cannot be hardcoded.
        assert_eq!(nth_sunday(2027, 3, 2), NaiveDate::from_ymd_opt(2027, 3, 14));
        assert_eq!(nth_sunday(2027, 11, 1), NaiveDate::from_ymd_opt(2027, 11, 7));
        assert_eq!(nth_sunday(2026, 3, 2), NaiveDate::from_ymd_opt(2026, 3, 8));
        assert_eq!(nth_sunday(2026, 11, 1), NaiveDate::from_ymd_opt(2026, 11, 1));
    }

    fn naive(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()
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

}
