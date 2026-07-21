"""Tests for open_paper_lots_from_forecasts.py"""
from __future__ import annotations

import json
import sqlite3
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from kalshi_ticker import ensure_forecasts_table  # noqa: E402
from open_paper_lots_from_forecasts import (  # noqa: E402
    SOURCE,
    build_candidates,
    ensure_paper_tables,
    fee_aware_edge,
    get_balance,
    open_lot,
    order_fee,
)


def _conn() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    ensure_forecasts_table(conn)
    ensure_paper_tables(conn)
    return conn


def _insert_fc(
    conn,
    *,
    ticker,
    created,
    p_market,
    p_model,
    p_final,
    verdict,
    breakdown=None,
    outcome=None,
):
    conn.execute(
        """
        INSERT INTO forecasts (
            market_ticker, created_at, close_time, p_market, p_model, p_final,
            verdict, verdict_reasons, agent_breakdown, outcome, resolved_at,
            event_key, is_in_play, source
        ) VALUES (?, ?, '2026-08-01T00:00:00Z', ?, ?, ?, ?, '[]', ?, ?, ?,
                  ?, 0, 'test')
        """,
        (
            ticker,
            created,
            p_market,
            p_model,
            p_final,
            verdict,
            breakdown,
            outcome,
            None if outcome is None else "2026-08-01T00:00:00Z",
            ticker.rsplit("-", 1)[0],
        ),
    )
    conn.commit()


def test_order_fee_matches_rust_shape():
    # 100 contracts at 0.50 with mult 0.07 → raw 1.75 → $1.75
    assert order_fee(0.50, 100.0, 0.07) == pytest.approx(1.75)


def test_fee_aware_edge_yes_positive():
    # model 0.40 market 0.20 fee 0.07 → entry ≈ 0.20+0.0112=0.2112; edge≈0.1888
    e = fee_aware_edge(0.40, 0.20, "YES", 0.07)
    assert e > 0.15


def test_skips_legacy_geometry_trade_yes():
    conn = _conn()
    _insert_fc(
        conn,
        ticker="KXBTC-26JUL2009-B64450",
        created="2026-07-20T12:00:00Z",
        p_market=0.14,
        p_model=0.69,
        p_final=0.28,
        verdict="trade_yes",
    )
    cands = build_candidates(
        conn, min_edge=0.05, fee_mult=0.07, daily_cap=5, limit=10
    )
    assert cands == []


def test_opens_trade_no_with_correct_side():
    conn = _conn()
    _insert_fc(
        conn,
        ticker="KXBTC-26JUL2214-T70000",
        created="2026-07-22T12:00:00Z",
        p_market=0.70,  # expensive YES → NO edge if model low
        p_model=0.40,
        p_final=0.45,
        verdict="trade_no",
        breakdown='{"contract":{"kind":"above"}}',
    )
    cands = build_candidates(
        conn, min_edge=0.05, fee_mult=0.07, daily_cap=5, limit=10
    )
    assert len(cands) == 1
    assert cands[0]["side"] == "NO"
    opened = open_lot(
        conn, cands[0], stake_cap=10.0, stake_pct=0.01, fee_mult=0.07
    )
    conn.commit()
    row = conn.execute("SELECT side, status, source FROM paper_lots").fetchone()
    assert row[0] == "NO"
    assert row[1] == "Open"
    assert row[2] == SOURCE
    assert opened["stake_dollars"] > 0
    assert get_balance(conn) < 10_000.0


def test_respects_daily_cap():
    conn = _conn()
    # Pre-seed 5 lots today from this source
    today = "2026-07-22T15:00:00+00:00"
    # Freeze "today" by writing lots with today's real UTC date via opened_at LIKE
    from datetime import datetime, timezone

    today_prefix = datetime.now(timezone.utc).strftime("%Y-%m-%d")
    for i in range(5):
        conn.execute(
            """
            INSERT INTO paper_lots
                (id, ticker, title, category, side, entry_price_cents, qty,
                 stake_dollars, source, opened_at, status)
            VALUES (?, ?, '', 'BTC', 'YES', 20, 1, 0.2, ?, ?, 'Open')
            """,
            (f"seed-{i}", f"SEED-{i}", SOURCE, f"{today_prefix}T10:0{i}:00+00:00"),
        )
    conn.commit()
    _insert_fc(
        conn,
        ticker="KXBTC-26JUL2214-B73250",
        created="2026-07-22T12:00:00Z",
        p_market=0.20,
        p_model=0.40,
        p_final=0.35,
        verdict="trade_yes",
        breakdown='{"contract":{"floor_strike":73200}}',
    )
    cands = build_candidates(
        conn, min_edge=0.05, fee_mult=0.07, daily_cap=5, limit=10
    )
    assert cands == []


def test_idempotent_on_second_run():
    conn = _conn()
    _insert_fc(
        conn,
        ticker="KXBTC-26JUL2214-B73250",
        created="2026-07-22T12:00:00Z",
        p_market=0.20,
        p_model=0.40,
        p_final=0.35,
        verdict="trade_yes",
        breakdown='{"contract":{"floor_strike":73200}}',
    )
    c1 = build_candidates(
        conn, min_edge=0.05, fee_mult=0.07, daily_cap=5, limit=10
    )
    assert len(c1) == 1
    open_lot(conn, c1[0], stake_cap=10.0, stake_pct=0.01, fee_mult=0.07)
    conn.commit()
    c2 = build_candidates(
        conn, min_edge=0.05, fee_mult=0.07, daily_cap=5, limit=10
    )
    assert c2 == []
    n = conn.execute("SELECT COUNT(*) FROM paper_lots").fetchone()[0]
    assert n == 1


def test_b_leg_outside_density_band_skipped():
    conn = _conn()
    _insert_fc(
        conn,
        ticker="KXBTC-26JUL2214-B50000",
        created="2026-07-22T12:00:00Z",
        p_market=0.02,  # lottery
        p_model=0.15,
        p_final=0.10,
        verdict="trade_yes",
        breakdown='{"contract":{"floor_strike":50000}}',
    )
    assert (
        build_candidates(
            conn, min_edge=0.05, fee_mult=0.07, daily_cap=5, limit=10
        )
        == []
    )
