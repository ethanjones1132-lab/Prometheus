"""Tests for scripts/kalshi_ticker.py.

These pin the same vectors as the Rust module's tests
(`kalshi-monster/src-tauri/src/kalshi/ticker.rs`). If the two ever disagree,
the cron scripts and the app will report different sample sizes for the same
database — which is the exact class of bug this module was written to close.
"""
from __future__ import annotations

import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from kalshi_ticker import (  # noqa: E402
    eligible_resolved_rows,
    ensure_provenance_columns,
    event_key,
    event_start_from_ticker,
    is_in_play,
    parse_timestamp,
    provenance_for,
)


def utc(y, m, d, h, mi):
    return datetime(y, m, d, h, mi, tzinfo=timezone.utc)


# --- event_key ---------------------------------------------------------------


def test_event_key_strips_the_final_strike_segment():
    assert (
        event_key("KXNBASUMMERSPREAD-26JUL11MIAORL-MIA10")
        == "KXNBASUMMERSPREAD-26JUL11MIAORL"
    )
    assert (
        event_key("KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR8")
        == "KXMLBTEAMTOTAL-26JUL191215CWSTOR"
    )
    assert event_key("KXWORLDCUPHALFTIME-26-POS") == "KXWORLDCUPHALFTIME-26"
    assert event_key("SENATETX-26-R") == "SENATETX-26"


def test_event_key_of_hyphenless_ticker_is_itself():
    assert event_key("KXTEST") == "KXTEST"
    assert event_key("") == ""


def test_correlated_legs_share_one_key():
    legs = [
        "KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR2",
        "KXMLBTEAMTOTAL-26JUL191215CWSTOR-CWS3",
        "KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR8",
    ]
    assert len({event_key(t) for t in legs}) == 1


# --- event_start_from_ticker -------------------------------------------------


def test_start_decomposes_as_year_month_day_time():
    assert event_start_from_ticker("KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR8") == utc(
        2026, 7, 19, 16, 15
    )
    assert event_start_from_ticker("KXMLBTEAMTOTAL-26JUL181510CINCOL-CIN4") == utc(
        2026, 7, 18, 19, 10
    )
    assert event_start_from_ticker("KXMLBTOTAL-26JUL171915CWSTOR-4") == utc(
        2026, 7, 17, 23, 15
    )


def test_eastern_evening_start_rolls_into_the_next_utc_day():
    assert event_start_from_ticker("KXMLBGAME-26JUL182008LADNYY-LAD") == utc(
        2026, 7, 19, 0, 8
    )


def test_tickers_without_an_encoded_time_return_none():
    # date only
    assert event_start_from_ticker("KXNBASUMMERTOTAL-26JUL11DENMIN-184") is None
    assert event_start_from_ticker("KXHIGHCHI-26JUL18-B89.5") is None
    # two-digit hour, not four
    assert event_start_from_ticker("KXBTC-26JUL1813-B64050") is None
    assert event_start_from_ticker("KXGOLDH-26JUL1612-T3979.99") is None
    # no date at all
    assert event_start_from_ticker("KXWORLDCUPHALFTIME-26-POS") is None
    assert event_start_from_ticker("KXTEST") is None
    assert event_start_from_ticker("") is None


def test_invalid_and_over_long_digit_runs_are_rejected():
    assert event_start_from_ticker("KXFOO-26JUL1912150-X") is None  # 12 digits
    assert event_start_from_ticker("KXFOO-26JUL192599ABC") is None  # 25:99
    assert event_start_from_ticker("KXFOO-26FEB301200ABC") is None  # Feb 30
    assert event_start_from_ticker("KXFOO-26XXX191215ABC") is None  # no such month
    assert event_start_from_ticker("KXFOO-26jul191215ABC") is None  # lowercase


# --- is_in_play --------------------------------------------------------------


def test_in_play_compares_creation_against_first_pitch():
    start = event_start_from_ticker("KXMLBTOTAL-26JUL171915CWSTOR-4")
    assert is_in_play("2026-07-18T02:30:00Z", start)
    assert not is_in_play("2026-07-17T18:00:00Z", start)
    assert is_in_play("2026-07-17T23:15:00Z", start), "exactly at start is live"


def test_unknown_start_is_never_in_play():
    assert not is_in_play("2026-07-18T02:30:00Z", None)


def test_parse_timestamp_accepts_every_ledger_spelling():
    # Rust to_rfc3339 (nanoseconds)
    assert parse_timestamp("2026-07-19T02:19:21.571103500+00:00") is not None
    # Python isoformat
    assert parse_timestamp("2026-07-16T14:08:27.389637+00:00") is not None
    # millisecond Z
    assert parse_timestamp("2026-07-19T02:19:21.571Z") is not None
    # zone-less is treated as UTC
    assert parse_timestamp("2026-07-19T02:19:21") == utc(2026, 7, 19, 2, 19).replace(
        second=21
    )
    assert parse_timestamp("") is None
    assert parse_timestamp("not a timestamp") is None


def test_provenance_for_returns_key_start_and_flag():
    key, start, in_play = provenance_for(
        "KXMLBTOTAL-26JUL171915CWSTOR-4", "2026-07-18T02:30:00Z"
    )
    assert key == "KXMLBTOTAL-26JUL171915CWSTOR"
    assert start.startswith("2026-07-17T23:15:00")
    assert in_play == 1


# --- eligible_resolved_rows --------------------------------------------------


def _ledger(rows):
    """In-memory forecasts table seeded with (ticker, created_at, p_model)."""
    conn = sqlite3.connect(":memory:")
    conn.execute(
        """
        CREATE TABLE forecasts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            market_ticker TEXT NOT NULL,
            created_at TEXT NOT NULL,
            close_time TEXT NOT NULL,
            p_market REAL NOT NULL,
            p_model REAL,
            p_final REAL NOT NULL,
            verdict TEXT NOT NULL,
            verdict_reasons TEXT NOT NULL,
            stake_suggested REAL,
            agent_breakdown TEXT,
            resolved_at TEXT,
            outcome INTEGER,
            brier_model REAL,
            brier_market REAL,
            brier_final REAL
        )
        """
    )
    ensure_provenance_columns(conn)
    for i, (ticker, created, p_model) in enumerate(rows):
        key, start, in_play = provenance_for(ticker, created)
        conn.execute(
            """
            INSERT INTO forecasts (
                market_ticker, created_at, close_time, p_market, p_model,
                p_final, verdict, verdict_reasons, resolved_at, outcome,
                event_start_at, is_in_play, source, event_key
            ) VALUES (?, ?, '2026-08-01T00:00:00Z', 0.5, ?, 0.5, 'pass', '[]',
                      ?, 1, ?, ?, 'test', ?)
            """,
            (ticker, created, p_model, f"2026-08-0{i % 9 + 1}T00:00:00Z",
             start, in_play, key),
        )
    conn.commit()
    return conn


def test_market_only_rows_are_never_eligible():
    conn = _ledger(
        [(f"KXMKT-26JUL11AAA{i}-B1", "2026-07-10T00:00:00Z", None) for i in range(50)]
    )
    assert eligible_resolved_rows(conn) == []


def test_in_play_rows_are_never_eligible():
    conn = _ledger(
        [
            # logged 3h after a 19:15 ET first pitch
            ("KXMLBTOTAL-26JUL171915CWSTOR-4", "2026-07-18T02:30:00Z", 0.9),
            # logged before it
            ("KXMLBTOTAL-26JUL191215CWSTOR-4", "2026-07-19T10:00:00Z", 0.6),
        ]
    )
    rows = eligible_resolved_rows(conn)
    assert len(rows) == 1
    assert rows[0][4] == "KXMLBTOTAL-26JUL191215CWSTOR"


def test_correlated_legs_collapse_to_one_observation():
    conn = _ledger(
        [
            (f"KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR{n}", "2026-07-19T10:00:00Z", 0.6)
            for n in range(2, 9)
        ]
    )
    assert len(eligible_resolved_rows(conn)) == 1


def test_the_audited_ledger_shape_yields_a_handful_not_hundreds():
    """258 market-only + 14 in-play legs + 5 correlated legs + 2 real rows."""
    rows = []
    rows += [
        (f"KXMKT-26JUL11AAA{i}-B1", "2026-07-10T00:00:00Z", None) for i in range(258)
    ]
    rows += [
        (f"KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR{n}", "2026-07-19T20:00:00Z", 0.9)
        for n in range(2, 16)
    ]
    rows += [
        (f"KXNBASUMMERSPREAD-26JUL11MIAORL-MIA{n}", "2026-07-10T04:00:00Z", 0.9)
        for n in (2, 4, 7, 10, 12)
    ]
    rows += [
        ("KXWNBASPREAD-26JUL09INDPHX-PHX13", "2026-07-08T04:00:00Z", 0.08),
        ("KXITFMATCH-26JUL10LOKALU-LOK", "2026-07-09T04:00:00Z", 0.86),
    ]
    conn = _ledger(rows)
    total = conn.execute("SELECT COUNT(*) FROM forecasts").fetchone()[0]
    assert total == 279
    assert len(eligible_resolved_rows(conn)) == 3


def test_ensure_provenance_columns_is_idempotent():
    conn = _ledger([("KXTEST-26JUL191215AAA-B1", "2026-07-19T10:00:00Z", 0.5)])
    ensure_provenance_columns(conn)
    ensure_provenance_columns(conn)
    cols = {r[1] for r in conn.execute("PRAGMA table_info(forecasts)")}
    for name in ("event_start_at", "is_in_play", "source", "event_key", "agents_opining"):
        assert name in cols
    assert conn.execute("SELECT COUNT(*) FROM forecasts").fetchone()[0] == 1
