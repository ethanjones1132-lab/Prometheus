"""Tests for settle_paper_lots.py"""
from __future__ import annotations

import sqlite3
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from kalshi_ticker import ensure_forecasts_table  # noqa: E402
from open_paper_lots_from_forecasts import (  # noqa: E402
    ensure_paper_tables,
    get_balance,
)
from settle_paper_lots import (  # noqa: E402
    close_lot_with_result,
    settlement_exit_cents_for_side,
    settle_open_lots,
)


def _conn() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    ensure_forecasts_table(conn)
    ensure_paper_tables(conn)
    return conn


def _insert_lot(conn, *, lot_id, ticker, side, entry_cents, qty, stake):
    conn.execute(
        """
        INSERT INTO paper_lots
            (id, ticker, title, category, side, entry_price_cents, qty,
             stake_dollars, source, opened_at, status)
        VALUES (?, ?, ?, 'BTC', ?, ?, ?, ?, 'test', '2026-07-22T00:00:00+00:00', 'Open')
        """,
        (lot_id, ticker, ticker, side, entry_cents, qty, stake),
    )
    # debit balance like opener
    conn.execute(
        "UPDATE paper_account SET balance_dollars = balance_dollars - ? WHERE id = 1",
        (stake,),
    )
    conn.commit()


def _insert_outcome(conn, ticker: str, outcome: int):
    conn.execute(
        """
        INSERT INTO forecasts (
            market_ticker, created_at, close_time, p_market, p_model, p_final,
            verdict, verdict_reasons, agent_breakdown, outcome, resolved_at,
            event_key, is_in_play, source
        ) VALUES (?, '2026-07-22T00:00:00+00:00', '2026-07-22T05:00:00Z',
                  0.2, 0.1, 0.15, 'trade_no', '[]', '{}', ?, '2026-07-22T05:01:00Z',
                  ?, 0, 'test')
        """,
        (ticker, outcome, ticker.rsplit("-", 1)[0]),
    )
    conn.commit()


def test_settlement_exit_side_aware():
    assert settlement_exit_cents_for_side("NO", "No") == 100.0
    assert settlement_exit_cents_for_side("NO", "Yes") == 0.0
    assert settlement_exit_cents_for_side("YES", "Yes") == 100.0
    assert settlement_exit_cents_for_side("YES", "No") == 0.0


def test_no_side_win_credits_full_payout():
    conn = _conn()
    # entry 84¢ NO, qty≈11.9, stake 10.12 → win pays qty*1.0
    qty = 11.904761904761905
    stake = 10.12
    bal0 = get_balance(conn)
    _insert_lot(
        conn,
        lot_id="lot-no-win",
        ticker="KXBTC-26JUL2204-B66450",
        side="NO",
        entry_cents=84.0,
        qty=qty,
        stake=stake,
    )
    _insert_outcome(conn, "KXBTC-26JUL2204-B66450", 0)  # Yes=0 → result No

    summary = settle_open_lots(conn, fetch_api=False, dry_run=False)
    assert summary["settled"] == 1
    assert summary["wins"] == 1
    assert summary["losses"] == 0
    d = summary["details"][0]
    assert d["exit_cents"] == 100.0
    assert d["realized_pnl"] == pytest.approx(qty - stake)
    # balance: bal0 - stake + proceeds(qty)
    assert get_balance(conn) == pytest.approx(bal0 - stake + qty)
    row = conn.execute(
        "SELECT status, settlement_result, realized_pnl FROM paper_lots WHERE id='lot-no-win'"
    ).fetchone()
    assert row[0] == "Closed"
    assert row[1] == "No"
    assert row[2] == pytest.approx(qty - stake)


def test_no_side_loss_zero_exit():
    conn = _conn()
    qty = 10.0
    stake = 6.1
    _insert_lot(
        conn,
        lot_id="lot-no-loss",
        ticker="KXBTC-TEST-B1",
        side="NO",
        entry_cents=61.0,
        qty=qty,
        stake=stake,
    )
    _insert_outcome(conn, "KXBTC-TEST-B1", 1)  # Yes

    summary = settle_open_lots(conn, fetch_api=False, dry_run=False)
    assert summary["settled"] == 1
    assert summary["losses"] == 1
    d = summary["details"][0]
    assert d["exit_cents"] == 0.0
    assert d["realized_pnl"] == pytest.approx(-stake)
    assert d["won"] is False


def test_dry_run_does_not_mutate():
    conn = _conn()
    bal0 = get_balance(conn)
    _insert_lot(
        conn,
        lot_id="lot-dry",
        ticker="KXINX-TEST-B1",
        side="NO",
        entry_cents=80.0,
        qty=12.0,
        stake=10.0,
    )
    bal_after_open = get_balance(conn)
    _insert_outcome(conn, "KXINX-TEST-B1", 0)

    summary = settle_open_lots(conn, fetch_api=False, dry_run=True)
    assert summary["mode"] == "dry_run"
    assert summary["settled"] == 1
    assert get_balance(conn) == pytest.approx(bal_after_open)
    status = conn.execute(
        "SELECT status FROM paper_lots WHERE id='lot-dry'"
    ).fetchone()[0]
    assert status == "Open"
    assert bal0 > bal_after_open


def test_skips_when_no_outcome():
    conn = _conn()
    _insert_lot(
        conn,
        lot_id="lot-skip",
        ticker="OPEN-TICKER",
        side="YES",
        entry_cents=40.0,
        qty=10.0,
        stake=4.0,
    )
    summary = settle_open_lots(conn, fetch_api=False, dry_run=False)
    assert summary["settled"] == 0
    assert summary["skipped_no_result"] == 1
    assert (
        conn.execute("SELECT status FROM paper_lots WHERE id='lot-skip'").fetchone()[0]
        == "Open"
    )


def test_close_lot_direct_math():
    conn = _conn()
    _insert_lot(
        conn,
        lot_id="lot-direct",
        ticker="T",
        side="YES",
        entry_cents=25.0,
        qty=40.0,
        stake=10.0,
    )
    lot = {
        "id": "lot-direct",
        "ticker": "T",
        "side": "YES",
        "qty": 40.0,
        "stake_dollars": 10.0,
        "status": "Open",
    }
    closed = close_lot_with_result(conn, lot, "Yes")
    conn.commit()
    assert closed["exit_cents"] == 100.0
    assert closed["proceeds"] == pytest.approx(40.0)
    assert closed["realized_pnl"] == pytest.approx(30.0)
