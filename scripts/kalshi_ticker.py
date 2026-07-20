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
from datetime import datetime, timedelta, timezone

# Kalshi embeds event times in US Eastern. This ledger is July data, where
# Eastern is EDT = UTC-4. See the Rust module for the EST caveat.
ET_TO_UTC_OFFSET_HOURS = 4

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


# --- Schema helper shared by the cron scripts --------------------------------

PROVENANCE_COLUMNS = (
    ("event_start_at", "TEXT"),
    ("is_in_play", "INTEGER DEFAULT 0"),
    ("source", "TEXT"),
    ("event_key", "TEXT"),
    ("agents_opining", "INTEGER"),
)


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
