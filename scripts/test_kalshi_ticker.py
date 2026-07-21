"""Tests for scripts/kalshi_ticker.py.

Parser behaviour is pinned by `scripts/ticker_vectors.json`, the same file the
Rust suite loads via `include_str!`. Both implementations iterate it, so a
change to one that the other does not match fails a test rather than quietly
making the app and the cron scripts report different sample sizes for the same
database. Add vectors to the JSON, not to one suite.
"""
from __future__ import annotations

import json
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import pytest  # noqa: E402

from kalshi_ticker import (  # noqa: E402
    _nth_sunday,
    eastern_is_edt,
    eligible_resolved_rows,
    ensure_provenance_columns,
    event_key,
    event_start_from_ticker,
    is_in_play,
    is_legacy_b_leg_model_row,
    parse_timestamp,
    provenance_for,
)

VECTORS = json.loads(
    (Path(__file__).resolve().parent / "ticker_vectors.json").read_text(encoding="utf-8")
)


def utc(y, m, d, h, mi):
    return datetime(y, m, d, h, mi, tzinfo=timezone.utc)


def _iso(s):
    return None if s is None else datetime.fromisoformat(s.replace("Z", "+00:00"))


def _id(case):
    return case.get("ticker") or case.get("input") or "<empty>"


# --- shared vectors ----------------------------------------------------------


def test_shared_vector_file_is_populated():
    """A silently truncated vector file would make every case below pass."""
    assert len(VECTORS["tickers"]) >= 25
    assert len(VECTORS["timestamps"]) >= 8


@pytest.mark.parametrize("case", VECTORS["tickers"], ids=_id)
def test_event_key_matches_shared_vector(case):
    assert event_key(case["ticker"]) == case["event_key"], case["note"]


@pytest.mark.parametrize("case", VECTORS["tickers"], ids=_id)
def test_event_start_matches_shared_vector(case):
    assert event_start_from_ticker(case["ticker"]) == _iso(
        case["event_start_utc"]
    ), case["note"]


@pytest.mark.parametrize("case", VECTORS["timestamps"], ids=_id)
def test_timestamp_parsing_matches_shared_vector(case):
    parsed = parse_timestamp(case["input"])
    expected = _iso(case["parsed_utc"])
    if expected is None:
        assert parsed is None, case["note"]
    else:
        assert parsed is not None and parsed.replace(microsecond=0) == expected, case["note"]


# --- behaviour the vector file cannot express --------------------------------


def test_correlated_legs_share_one_key():
    legs = [
        "KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR2",
        "KXMLBTEAMTOTAL-26JUL191215CWSTOR-CWS3",
        "KXMLBTEAMTOTAL-26JUL191215CWSTOR-TOR8",
    ]
    assert len({event_key(t) for t in legs}) == 1


def test_event_key_separates_the_two_maps_of_one_esports_series():
    assert event_key("KXVALORANTMAP-26JUL101900SRNRGA-1-NRGA") != event_key(
        "KXVALORANTMAP-26JUL101900SRNRGA-2-SR"
    )


# --- daylight saving window --------------------------------------------------


def test_dst_boundaries_are_exact_for_2026():
    # 2026: DST runs 8 March 02:00 -> 1 November 02:00 (Eastern).
    assert eastern_is_edt(datetime(2026, 3, 8, 1, 59)) is False
    assert eastern_is_edt(datetime(2026, 3, 8, 2, 0)) is True
    assert eastern_is_edt(datetime(2026, 11, 1, 1, 59)) is True
    assert eastern_is_edt(datetime(2026, 11, 1, 2, 0)) is False


def test_dst_window_tracks_the_calendar_not_a_fixed_date():
    from datetime import date

    assert _nth_sunday(2027, 3, 2) == date(2027, 3, 14)
    assert _nth_sunday(2027, 11, 1) == date(2027, 11, 7)
    assert _nth_sunday(2026, 3, 2) == date(2026, 3, 8)
    assert _nth_sunday(2026, 11, 1) == date(2026, 11, 1)


# --- is_in_play --------------------------------------------------------------


def test_in_play_compares_creation_against_first_pitch():
    start = event_start_from_ticker("KXMLBTOTAL-26JUL171915CWSTOR-4")
    assert is_in_play("2026-07-18T02:30:00Z", start)
    assert not is_in_play("2026-07-17T18:00:00Z", start)
    assert is_in_play("2026-07-17T23:15:00Z", start), "exactly at start is live"


def test_unknown_start_is_never_in_play():
    assert not is_in_play("2026-07-18T02:30:00Z", None)


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


def test_legacy_b_leg_model_rows_are_never_eligible():
    """Pre-bracket-fix B-legs used P(S>K) and must not enter the gate."""
    assert is_legacy_b_leg_model_row(
        "KXBTC-26JUL2009-B64450", "2026-07-20T12:00:00Z", 0.69, None
    )
    assert not is_legacy_b_leg_model_row(
        "KXBTC-26JUL2009-B64450",
        "2026-07-20T12:00:00Z",
        0.69,
        '{"contract":{"floor_strike":64400}}',
    )
    assert not is_legacy_b_leg_model_row(
        "KXBTC-26JUL2114-T73299", "2026-07-20T12:00:00Z", 0.40, None
    )
    assert not is_legacy_b_leg_model_row(
        "KXBTC-26JUL2114-B73250", "2026-07-22T12:00:00Z", 0.12, None
    )

    conn = _ledger(
        [
            # legacy poisoned B-leg
            ("KXBTC-26JUL2009-B64450", "2026-07-20T12:00:00Z", 0.69),
            # post-fix B-leg (after cutoff)
            ("KXBTC-26JUL2214-B73250", "2026-07-22T12:00:00Z", 0.12),
            # T-leg always OK
            ("KXBTC-26JUL2009-T65000", "2026-07-20T12:00:00Z", 0.40),
        ]
    )
    keys = {r[4] for r in eligible_resolved_rows(conn)}
    assert "KXBTC-26JUL2009-B64450" not in (keys or set())
    # event keys drop final segment
    assert any(k and "B73250" not in k and "JUL22" in (k or "") for k in keys) or any(
        "JUL22" in (k or "") for k in keys
    )
    assert len(eligible_resolved_rows(conn)) == 2


def test_duplicate_guard_catches_an_immediate_re_run():
    from kalshi_ticker import find_duplicate_forecast

    conn = _ledger([("KXTEST-26JUL191215AAA-B1", "2026-07-19T10:00:00Z", 0.5)])
    conn.execute("UPDATE forecasts SET p_market = 0.42")
    conn.commit()
    assert (
        find_duplicate_forecast(conn, "KXTEST-26JUL191215AAA-B1", 0.42, "2026-07-19T10:00:30Z")
        is not None
    )
    # Outside the window, a moved price, or another ticker are all genuine rows.
    assert (
        find_duplicate_forecast(conn, "KXTEST-26JUL191215AAA-B1", 0.42, "2026-07-19T10:01:01Z")
        is None
    )
    assert (
        find_duplicate_forecast(conn, "KXTEST-26JUL191215AAA-B1", 0.55, "2026-07-19T10:00:30Z")
        is None
    )
    assert (
        find_duplicate_forecast(conn, "KXOTHER-26JUL191215AAA-B1", 0.42, "2026-07-19T10:00:30Z")
        is None
    )


def test_duplicate_guard_fails_open_on_an_unparseable_timestamp():
    """`julianday()` returns NULL on garbage, so the comparison matches nothing
    and the insert proceeds. A missed suppression costs one extra row; a false
    one would silently discard a real forecast."""
    from kalshi_ticker import find_duplicate_forecast

    conn = _ledger([("KXTEST-26JUL191215AAA-B1", "2026-07-19T10:00:00Z", 0.5)])
    conn.execute("UPDATE forecasts SET p_market = 0.42, created_at = 'not a timestamp'")
    conn.commit()
    assert (
        find_duplicate_forecast(conn, "KXTEST-26JUL191215AAA-B1", 0.42, "2026-07-19T10:00:30Z")
        is None
    )


def test_ensure_forecasts_table_creates_the_full_schema():
    from kalshi_ticker import ensure_forecasts_table

    conn = sqlite3.connect(":memory:")
    ensure_forecasts_table(conn)
    ensure_forecasts_table(conn)  # idempotent
    cols = {r[1] for r in conn.execute("PRAGMA table_info(forecasts)")}
    for name in (
        "market_ticker", "p_market", "p_model", "p_final", "outcome",
        "event_start_at", "is_in_play", "source", "event_key", "agents_opining",
    ):
        assert name in cols


def test_ensure_provenance_columns_is_idempotent():
    conn = _ledger([("KXTEST-26JUL191215AAA-B1", "2026-07-19T10:00:00Z", 0.5)])
    ensure_provenance_columns(conn)
    ensure_provenance_columns(conn)
    cols = {r[1] for r in conn.execute("PRAGMA table_info(forecasts)")}
    for name in ("event_start_at", "is_in_play", "source", "event_key", "agents_opining"):
        assert name in cols
    assert conn.execute("SELECT COUNT(*) FROM forecasts").fetchone()[0] == 1
