#!/usr/bin/env python3
"""Kalshi ticker structure — Python mirror of
`kalshi-monster/src-tauri/src/kalshi/ticker.rs`.

Kept in lockstep with the Rust module: the cron scripts and the app must agree
on which ledger rows count as evidence, or the two will report different
sample sizes for the same database.

Two facts are recoverable from a ticker:

1. ``event_key`` — the ticker minus its final ``-SEGMENT`` (the strike /
   outcome leg). Ten legs of one baseball game are ten ledger rows but one
   observation of the world.
2. ``event_start_from_ticker`` — the UTC instant the underlying event began,
   parsed from an embedded ``YYMMMDDHHMM`` (Eastern). A forecast written after
   that instant copied a price that already contained the score.

Both are total and never guess: no encoded time means ``None``, not midnight.
"""
from __future__ import annotations

import re
from datetime import date, datetime, time, timedelta, timezone

# Kalshi embeds event times in US Eastern. During EDT (roughly mid-March to
# early November) that is UTC-4. This is a fixed offset, not a timezone
# database lookup, so it is only applied inside the EDT window: outside it the
# true offset is 5h and this constant would place the event one hour EARLIER
# than it really was, so a forecast written during that hour looks like it came
# after the start when it came before. `event_start_from_ticker` returns None
# there instead. See kalshi/ticker.rs for the full reasoning.
ET_TO_UTC_OFFSET_HOURS = 4


def _nth_sunday(year: int, month: int, n: int) -> date:
    """The nth Sunday (1-based) of the given month."""
    first = date(year, month, 1)
    # date.weekday() is Mon=0; convert to days since Sunday.
    first_sunday = 1 + (7 - ((first.weekday() + 1) % 7)) % 7
    return date(year, month, first_sunday + 7 * (n - 1))


def eastern_is_edt(naive: datetime) -> bool:
    """True when a US Eastern wall clock falls inside daylight saving time:
    second Sunday of March 02:00 through first Sunday of November 02:00."""
    y = naive.year
    start = datetime.combine(_nth_sunday(y, 3, 2), time(2, 0))
    end = datetime.combine(_nth_sunday(y, 11, 1), time(2, 0))
    return start <= naive.replace(tzinfo=None) < end

_MONTHS = {
    m: i + 1
    for i, m in enumerate(
        ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP",
         "OCT", "NOV", "DEC"]
    )
}

# YY MMM DD HHMM, and the digit run must stop there — a 12th digit means these
# eleven characters are a prefix of something else, not a timestamp.
_DATETIME_RE = re.compile(r"^(\d{2})([A-Z]{3})(\d{2})(\d{2})(\d{2})(?![0-9])")

# A usable timestamp must carry a time of day: `YYYY-MM-DD` alone does not.
_HAS_TIME_RE = re.compile(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}")


def event_key(ticker: str) -> str:
    """The ticker with its final ``-SEGMENT`` removed.

    ``KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR8`` -> ``KXMLBTEAMTOTAL-26JUL191215CWSTOR``
    A ticker with no hyphen is returned unchanged.
    """
    head, sep, _tail = ticker.rpartition("-")
    return head if (sep and head) else ticker


def event_start_from_ticker(ticker: str) -> datetime | None:
    """UTC start of the underlying event, or ``None`` if none is encoded.

    ``None`` covers date-only tickers (``-26JUL11DENMIN``), truncated hours
    (``KXBTC-26JUL1813`` — two digits, not four), and tickers with no date.

    Also ``None`` for dates outside the EDT window: an instant this module
    knows is an hour wrong is worse than an admitted unknown, because
    ``is_in_play`` is computed once at insert and stored.
    """
    for seg in ticker.split("-"):
        m = _DATETIME_RE.match(seg)
        if not m:
            continue
        yy, mon, dd, hh, mi = m.groups()
        month = _MONTHS.get(mon)
        if month is None:
            continue
        try:
            naive = datetime(
                2000 + int(yy), month, int(dd), int(hh), int(mi), tzinfo=timezone.utc
            )
        except ValueError:
            continue
        if not eastern_is_edt(naive):
            return None
        return naive + timedelta(hours=ET_TO_UTC_OFFSET_HOURS)
    return None


def parse_timestamp(s: str) -> datetime | None:
    """Parse any of the timestamp spellings the ledger contains.

    Rows have been written by Rust ``to_rfc3339`` (nanosecond precision),
    Python ``isoformat``, and a millisecond ``Z`` form.
    """
    if not s:
        return None
    text = s.strip().replace("Z", "+00:00")
    # datetime.fromisoformat rejects more than 6 fractional digits.
    text = re.sub(r"(\.\d{6})\d+", r"\1", text)
    # A date with no time of day carries no ordering against an event start,
    # and fromisoformat would silently supply midnight. Rust returns None here;
    # accepting it would put the two implementations out of step.
    if not _HAS_TIME_RE.search(text):
        return None
    try:
        dt = datetime.fromisoformat(text)
    except ValueError:
        return None
    return dt if dt.tzinfo else dt.replace(tzinfo=timezone.utc)


def is_in_play(created_at: str, event_start: datetime | None) -> bool:
    """True when the row was written at or after the event began.

    False when the start time is unknown: absence of evidence is not evidence
    of in-play, and it is recorded as a NULL ``event_start_at``.
    """
    created = parse_timestamp(created_at)
    if created is None or event_start is None:
        return False
    return created >= event_start


def provenance_for(ticker: str, created_at: str) -> tuple[str, str | None, int]:
    """``(event_key, event_start_at_iso_or_None, is_in_play_int)``."""
    start = event_start_from_ticker(ticker)
    return (
        event_key(ticker),
        start.isoformat() if start else None,
        1 if is_in_play(created_at, start) else 0,
    )


# --- Schema and ledger helpers shared by every cron script -------------------
#
# These live here for the same reason `FORECASTS_DDL` was extracted on the Rust
# side: three scripts write to this table, and hand-copying a schema or a
# threshold into each of them is how they drift apart.

# Mirrors GateConfig::default().min_resolved in edge_engine/calibration.rs.
MIN_ELIGIBLE_FOR_GATE = 200

# Mirrors DUPLICATE_SUPPRESSION_SECS in kalshi/forecast.rs. See
# `find_duplicate_forecast` for what this window does and does not catch.
DUPLICATE_SUPPRESSION_SECS = 60

PROVENANCE_COLUMNS = (
    ("event_start_at", "TEXT"),
    ("is_in_play", "INTEGER DEFAULT 0"),
    ("source", "TEXT"),
    ("event_key", "TEXT"),
    ("agents_opining", "INTEGER"),
)

# Mirrors `FORECASTS_DDL` in kalshi/forecast.rs. Kept in one place so adding a
# column does not mean hand-editing three inlined copies.
FORECASTS_DDL = """
    CREATE TABLE IF NOT EXISTS forecasts (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        market_ticker   TEXT NOT NULL,
        created_at      TEXT NOT NULL,
        close_time      TEXT NOT NULL,
        p_market        REAL NOT NULL,
        p_model         REAL,
        p_final         REAL NOT NULL,
        verdict         TEXT NOT NULL,
        verdict_reasons TEXT NOT NULL,
        stake_suggested REAL,
        agent_breakdown TEXT,

        resolved_at     TEXT,
        outcome         INTEGER,
        brier_model     REAL,
        brier_market    REAL,
        brier_final     REAL,

        event_start_at  TEXT,
        is_in_play      INTEGER DEFAULT 0,
        source          TEXT,
        event_key       TEXT,
        agents_opining  INTEGER
    )
"""

# Read-then-insert duplicate lookup. `julianday` is used rather than string
# comparison because the ledger holds three timestamp spellings that do not
# sort against each other reliably.
DUPLICATE_LOOKUP_SQL = """
    SELECT id FROM forecasts
    WHERE market_ticker = ?
      AND ABS(p_market - ?) < 1e-9
      AND ABS(julianday(created_at) - julianday(?)) * 86400.0 <= ?
    ORDER BY id DESC LIMIT 1
"""


def ensure_forecasts_table(conn) -> None:
    """Create the forecasts table if absent and bring an older one up to the
    migration-4 schema. Idempotent; safe to call on every run."""
    conn.execute(FORECASTS_DDL)
    conn.commit()
    ensure_provenance_columns(conn)


def ensure_provenance_columns(conn) -> None:
    """Add the migration-4 provenance columns if this script runs against a
    database the app has not migrated yet. Mirrors
    ``migration_04_forecast_provenance_columns_tx``; idempotent.
    """
    existing = {r[1] for r in conn.execute("PRAGMA table_info(forecasts)")}
    if not existing:
        return  # no forecasts table yet; the caller creates it
    for name, ty in PROVENANCE_COLUMNS:
        if name not in existing:
            conn.execute(f"ALTER TABLE forecasts ADD COLUMN {name} {ty}")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_fc_event_key ON forecasts(event_key)")
    conn.commit()


def find_duplicate_forecast(conn, ticker: str, p_market: float, created_at: str):
    """Id of an existing row this insert would duplicate, or ``None``.

    Mirrors the guard in `kalshi::forecast::insert_forecast`, with the same
    limits: it is **advisory**, not a constraint. The lookup is a separate
    statement from the insert, so two concurrent writers can both miss; and it
    **fails open** when SQLite's `julianday()` cannot parse a stored
    `created_at`, because a NULL comparison matches nothing.

    What the 60s window actually catches is double-submits and immediate
    re-runs. It is deliberately not wide enough to collapse the ledger's
    existing duplicate tickers — those sit 0-950s apart, and a 950s window
    would also swallow genuine re-quotes of a market whose price simply has
    not moved. Correlated and repeated rows are neutralised for measurement by
    `eligible_resolved_rows`' dedup instead, which does not have to guess.
    """
    row = conn.execute(
        DUPLICATE_LOOKUP_SQL,
        (ticker, p_market, created_at, DUPLICATE_SUPPRESSION_SECS),
    ).fetchone()
    return int(row[0]) if row else None


def eligible_resolved_rows(conn) -> list[tuple]:
    """Resolved rows that can testify to *model* skill, one per event.

    Mirrors ``edge_engine::calibration::eligible_rows``:
    ``p_model`` present, not in play, deduplicated by ``event_key``.
    Returns ``(p_market, p_model, p_final, outcome, event_key)`` tuples in
    resolution order.
    """
    rows = conn.execute(
        """
        SELECT p_market, p_model, p_final, outcome, event_key, COALESCE(is_in_play, 0)
        FROM forecasts
        WHERE outcome IS NOT NULL AND p_model IS NOT NULL
        ORDER BY resolved_at ASC, id ASC
        """
    ).fetchall()
    seen: set[str] = set()
    out: list[tuple] = []
    for p_market, p_model, p_final, outcome, key, in_play in rows:
        if in_play:
            continue
        if key is not None:
            if key in seen:
                continue
            seen.add(key)
        out.append((p_market, p_model, p_final, outcome, key))
    return out
