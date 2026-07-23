#!/usr/bin/env python3
"""Settle open paper_lots against forecast ledger outcomes (or Kalshi API).

Cron companion to open_paper_lots_from_forecasts.py. Mirrors Rust
`paper::settle_pending` + `close_lot_with_result` money math so the paper
PnL leg of the Phase 3 gate can advance without the desktop app running.

Resolution order per open lot ticker:
  1. forecasts.outcome (0/1) already written by resolve_settled_forecasts.py
  2. Optional --fetch-api: GET market result from public Kalshi API
  3. Skip if still unknown

Exit price is held-side: YES+Yes or NO+No → 100¢ else 0¢.
realized_pnl = qty * exit/100 - stake_dollars; balance credited proceeds.
"""
from __future__ import annotations

import argparse
import json
import os
import sqlite3
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from open_paper_lots_from_forecasts import (  # noqa: E402
    DEFAULT_DB,
    ensure_paper_tables,
    get_balance,
)

KALSHI_MARKET_URL = (
    "https://api.elections.kalshi.com/trade-api/v2/markets/{ticker}"
)
SOURCE = "settle_paper_lots.py"


def settlement_exit_cents_for_side(side: str, actual_yes_no: str) -> float:
    """Mirror Rust paper::settlement_exit_cents_for_side."""
    s = (side or "").strip().upper()
    a = (actual_yes_no or "").strip()
    held_wins = (s == "YES" and a.lower() == "yes") or (
        s == "NO" and a.lower() == "no"
    )
    return 100.0 if held_wins else 0.0


def normalize_settlement_result(raw: str | None) -> str | None:
    if raw is None:
        return None
    t = str(raw).strip()
    if not t:
        return None
    low = t.lower()
    if low in ("yes", "y", "true", "1"):
        return "Yes"
    if low in ("no", "n", "false", "0"):
        return "No"
    if t in ("Yes", "No"):
        return t
    return None


def outcome_int_to_yes_no(outcome: int | float | None) -> str | None:
    if outcome is None:
        return None
    try:
        v = int(outcome)
    except (TypeError, ValueError):
        return None
    if v == 1:
        return "Yes"
    if v == 0:
        return "No"
    return None


def lookup_result_from_forecasts(conn: sqlite3.Connection, ticker: str) -> str | None:
    row = conn.execute(
        """
        SELECT outcome FROM forecasts
        WHERE market_ticker = ? AND outcome IS NOT NULL
        ORDER BY resolved_at DESC, id DESC
        LIMIT 1
        """,
        (ticker,),
    ).fetchone()
    if not row:
        return None
    return outcome_int_to_yes_no(row[0])


def fetch_result_from_api(ticker: str, timeout: float = 20.0) -> str | None:
    url = KALSHI_MARKET_URL.format(ticker=ticker)
    req = urllib.request.Request(
        url,
        headers={"Accept": "application/json", "User-Agent": "kalshi-monster-settle/1.0"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = json.loads(resp.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, OSError):
        return None
    market = body.get("market") or body
    return normalize_settlement_result(market.get("result") or "")


def open_lots(conn: sqlite3.Connection) -> list[dict]:
    cur = conn.execute(
        """
        SELECT id, ticker, side, entry_price_cents, qty, stake_dollars, status
        FROM paper_lots
        WHERE status = 'Open'
        ORDER BY opened_at ASC
        """
    )
    cols = [d[0] for d in cur.description]
    return [dict(zip(cols, row)) for row in cur.fetchall()]


def close_lot_with_result(
    conn: sqlite3.Connection,
    lot: dict,
    actual: str,
    *,
    now: str | None = None,
) -> dict:
    """Mirror Rust close_lot_with_result money math."""
    if lot["status"] != "Open":
        raise RuntimeError(f"lot {lot['id']} is not Open")
    actual_n = normalize_settlement_result(actual)
    if actual_n not in ("Yes", "No"):
        raise RuntimeError(f"bad settlement result {actual!r}")

    exit_cents = settlement_exit_cents_for_side(lot["side"], actual_n)
    qty = float(lot["qty"])
    stake = float(lot["stake_dollars"])
    proceeds = qty * exit_cents / 100.0
    realized = proceeds - stake
    ts = now or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S+00:00")

    conn.execute(
        """
        UPDATE paper_lots
        SET closed_at = ?, closed_price_cents = ?, realized_pnl = ?,
            status = 'Closed', settlement_result = ?
        WHERE id = ? AND status = 'Open'
        """,
        (ts, exit_cents, realized, actual_n, lot["id"]),
    )
    if conn.total_changes < 1:
        raise RuntimeError(f"failed to close lot {lot['id']}")

    conn.execute(
        """
        UPDATE paper_account
        SET balance_dollars = balance_dollars + ?, updated_at = ?
        WHERE id = 1
        """,
        (proceeds, ts),
    )

    won = (str(lot["side"]).upper() == "YES" and actual_n == "Yes") or (
        str(lot["side"]).upper() == "NO" and actual_n == "No"
    )
    return {
        "lot_id": lot["id"],
        "ticker": lot["ticker"],
        "side": lot["side"],
        "actual": actual_n,
        "exit_cents": exit_cents,
        "proceeds": proceeds,
        "realized_pnl": realized,
        "won": won,
        "closed_at": ts,
        "status": "settled",
    }


def settle_open_lots(
    conn: sqlite3.Connection,
    *,
    fetch_api: bool = False,
    dry_run: bool = False,
) -> dict:
    ensure_paper_tables(conn)
    lots = open_lots(conn)
    details: list[dict] = []
    settled = wins = losses = 0
    total_pnl = 0.0
    skipped = 0

    # Cache ticker → Yes/No
    result_cache: dict[str, str | None] = {}

    for lot in lots:
        ticker = lot["ticker"]
        if ticker not in result_cache:
            result = lookup_result_from_forecasts(conn, ticker)
            if result is None and fetch_api:
                result = fetch_result_from_api(ticker)
            result_cache[ticker] = result
        actual = result_cache[ticker]
        if actual is None:
            skipped += 1
            details.append(
                {
                    "lot_id": lot["id"],
                    "ticker": ticker,
                    "side": lot["side"],
                    "status": "skipped_no_result",
                }
            )
            continue

        if dry_run:
            exit_cents = settlement_exit_cents_for_side(lot["side"], actual)
            proceeds = float(lot["qty"]) * exit_cents / 100.0
            realized = proceeds - float(lot["stake_dollars"])
            won = (str(lot["side"]).upper() == "YES" and actual == "Yes") or (
                str(lot["side"]).upper() == "NO" and actual == "No"
            )
            details.append(
                {
                    "lot_id": lot["id"],
                    "ticker": ticker,
                    "side": lot["side"],
                    "actual": actual,
                    "exit_cents": exit_cents,
                    "realized_pnl": realized,
                    "won": won,
                    "status": "dry_run",
                }
            )
            settled += 1
            total_pnl += realized
            if won:
                wins += 1
            else:
                losses += 1
            continue

        closed = close_lot_with_result(conn, lot, actual)
        details.append(closed)
        settled += 1
        total_pnl += closed["realized_pnl"]
        if closed["won"]:
            wins += 1
        else:
            losses += 1

    if not dry_run:
        conn.commit()
    else:
        conn.rollback()

    return {
        "mode": "dry_run" if dry_run else "execute",
        "source": SOURCE,
        "open_before": len(lots),
        "settled": settled,
        "wins": wins,
        "losses": losses,
        "skipped_no_result": skipped,
        "total_pnl": total_pnl,
        "balance": get_balance(conn),
        "details": details,
    }


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", type=Path, default=Path(os.environ.get("KALSHI_DB", DEFAULT_DB)))
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true", default=False)
    mode.add_argument("--execute", action="store_true", default=True)
    ap.add_argument(
        "--fetch-api",
        action="store_true",
        help="If forecast ledger has no outcome, fetch market.result from Kalshi public API",
    )
    args = ap.parse_args(argv)
    # default execute unless --dry-run
    dry_run = bool(args.dry_run)

    if not args.db.exists():
        print(f"DB missing: {args.db}", file=sys.stderr)
        return 2

    conn = sqlite3.connect(str(args.db))
    try:
        summary = settle_open_lots(
            conn, fetch_api=bool(args.fetch_api), dry_run=dry_run
        )
    finally:
        conn.close()

    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
