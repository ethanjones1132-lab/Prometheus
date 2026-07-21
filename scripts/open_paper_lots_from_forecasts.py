#!/usr/bin/env python3
"""Open small paper lots from post-bracket-fix trade_yes / trade_no forecasts.

Cron-friendly path into the same `paper_lots` / `paper_account` tables the
Tauri app uses. Does NOT enable live trading.

Defaults:
  --dry-run   print candidates only
  --execute   write lots + debit cash

Selection rules (v1):
  - created_at >= BRACKET_GEOMETRY_FIX_CUTOFF (or breakdown has floor/cap)
  - not legacy B-leg geometry
  - verdict in {trade_yes, trade_no}
  - p_model present, outcome NULL (still open)
  - fee-aware net edge >= min_edge
  - market mid in (0.05, 0.95); B-legs prefer (0.08, 0.45)
  - no existing open lot on same ticker+side
  - stake = min($10, 1% of paper balance)
  - daily cap 5 new lots
"""
from __future__ import annotations

import argparse
import json
import math
import os
import sqlite3
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from kalshi_ticker import (  # noqa: E402
    BRACKET_GEOMETRY_FIX_CUTOFF,
    is_b_leg_ticker,
    is_legacy_b_leg_model_row,
    parse_timestamp,
)

DEFAULT_DB = (
    Path(os.environ.get("USERPROFILE", os.environ.get("HOME", ".")))
    / ".openclaw/kalshi-monster/predictions.db"
)
DEFAULT_STARTING_BALANCE = 10_000.0
DEFAULT_FEE_MULT = 0.07
DEFAULT_MIN_EDGE = 0.05
DEFAULT_STAKE_CAP = 10.0
DEFAULT_STAKE_PCT = 0.01
DEFAULT_DAILY_CAP = 5
SOURCE = "open_paper_lots_from_forecasts.py"


def order_fee(p: float, contracts: float, fee_multiplier: float) -> float:
    """Mirror Rust edge_engine::order_fee (ceil to next cent)."""
    p = max(0.0, min(1.0, p))
    raw = fee_multiplier * p * (1.0 - p) * contracts
    return max(0.0, math.ceil(raw * 100.0 - 1e-9) / 100.0)


def ensure_paper_tables(conn: sqlite3.Connection) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS paper_account (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            balance_dollars REAL NOT NULL,
            total_deposits REAL NOT NULL DEFAULT 0,
            total_withdrawals REAL NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS paper_lots (
            id TEXT PRIMARY KEY,
            ticker TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            category TEXT NOT NULL DEFAULT 'Other',
            side TEXT NOT NULL,
            entry_price_cents REAL NOT NULL,
            qty REAL NOT NULL,
            stake_dollars REAL NOT NULL,
            source TEXT NOT NULL DEFAULT 'Manual',
            decision_json TEXT,
            opened_at TEXT NOT NULL,
            closed_at TEXT,
            closed_price_cents REAL,
            realized_pnl REAL,
            status TEXT NOT NULL DEFAULT 'Open',
            settlement_result TEXT,
            prediction_id TEXT
        )
        """
    )
    # prediction_id may already exist from app migration
    cols = {r[1] for r in conn.execute("PRAGMA table_info(paper_lots)")}
    if "prediction_id" not in cols:
        conn.execute("ALTER TABLE paper_lots ADD COLUMN prediction_id TEXT")
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_paper_lots_ticker ON paper_lots(ticker)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_paper_lots_status ON paper_lots(status)"
    )
    exists = conn.execute(
        "SELECT EXISTS(SELECT 1 FROM paper_account WHERE id = 1)"
    ).fetchone()[0]
    if not exists:
        now = datetime.now(timezone.utc).isoformat()
        conn.execute(
            """
            INSERT INTO paper_account
                (id, balance_dollars, total_deposits, total_withdrawals, created_at, updated_at)
            VALUES (1, ?, ?, 0, ?, ?)
            """,
            (DEFAULT_STARTING_BALANCE, DEFAULT_STARTING_BALANCE, now, now),
        )
    conn.commit()


def get_balance(conn: sqlite3.Connection) -> float:
    row = conn.execute(
        "SELECT balance_dollars FROM paper_account WHERE id = 1"
    ).fetchone()
    return float(row[0]) if row else DEFAULT_STARTING_BALANCE


def lots_opened_today(conn: sqlite3.Connection) -> int:
    # opened_at ISO; compare date prefix UTC
    today = datetime.now(timezone.utc).strftime("%Y-%m-%d")
    row = conn.execute(
        """
        SELECT COUNT(*) FROM paper_lots
        WHERE source = ? AND opened_at LIKE ?
        """,
        (SOURCE, f"{today}%"),
    ).fetchone()
    return int(row[0] or 0)


def has_open_lot(conn: sqlite3.Connection, ticker: str, side: str) -> bool:
    row = conn.execute(
        """
        SELECT 1 FROM paper_lots
        WHERE ticker = ? AND side = ? AND status = 'Open'
        LIMIT 1
        """,
        (ticker, side),
    ).fetchone()
    return row is not None


def fee_aware_edge(p_final: float, p_market: float, side: str, fee_mult: float) -> float:
    """Net edge after fee at mid, YES or NO."""
    p_m = max(0.01, min(0.99, p_market))
    p_f = max(0.01, min(0.99, p_final))
    if side == "YES":
        entry = p_m + fee_mult * p_m * (1.0 - p_m)
        return p_f - entry
    # NO: model P(NO)=1-p_f, market NO mid ≈ 1-p_m
    m_no = 1.0 - p_m
    entry = m_no + fee_mult * m_no * (1.0 - m_no)
    return (1.0 - p_f) - entry


def category_from_ticker(ticker: str) -> str:
    head = (ticker or "").split("-", 1)[0]
    if head.startswith("KX"):
        return head[2:] or "Other"
    return head or "Other"


def candidate_rows(conn: sqlite3.Connection) -> list[sqlite3.Row]:
    conn.row_factory = sqlite3.Row
    return list(
        conn.execute(
            """
            SELECT id, market_ticker, created_at, close_time, p_market, p_model,
                   p_final, verdict, verdict_reasons, agent_breakdown, stake_suggested
            FROM forecasts
            WHERE outcome IS NULL
              AND p_model IS NOT NULL
              AND verdict IN ('trade_yes', 'trade_no')
            ORDER BY id DESC
            """
        )
    )


def is_post_fix_ok(row: sqlite3.Row) -> bool:
    ticker = row["market_ticker"]
    created = row["created_at"]
    breakdown = row["agent_breakdown"]
    if is_legacy_b_leg_model_row(ticker, created, row["p_model"], breakdown):
        return False
    # Explicit post-fix: after cutoff OR has contract geometry in breakdown
    if breakdown and (
        '"floor_strike"' in breakdown or '"cap_strike"' in breakdown
    ):
        return True
    dt = parse_timestamp(created or "")
    return dt is not None and dt >= BRACKET_GEOMETRY_FIX_CUTOFF


def mid_ok(ticker: str, p_market: float) -> bool:
    if not (0.05 < p_market < 0.95):
        return False
    if is_b_leg_ticker(ticker):
        # Prefer density near spot — skip lottery deep OTM bins
        return 0.08 <= p_market <= 0.45
    return True


def build_candidates(
    conn: sqlite3.Connection,
    *,
    min_edge: float,
    fee_mult: float,
    daily_cap: int,
    limit: int,
) -> list[dict]:
    opened = lots_opened_today(conn)
    remaining = max(0, daily_cap - opened)
    out: list[dict] = []
    for row in candidate_rows(conn):
        if len(out) >= limit or remaining <= 0:
            break
        if not is_post_fix_ok(row):
            continue
        ticker = row["market_ticker"]
        p_market = float(row["p_market"])
        p_final = float(row["p_final"])
        verdict = row["verdict"]
        side = "YES" if verdict == "trade_yes" else "NO"
        if not mid_ok(ticker, p_market):
            continue
        edge = fee_aware_edge(p_final, p_market, side, fee_mult)
        if edge < min_edge:
            continue
        if has_open_lot(conn, ticker, side):
            continue
        entry_cents = (
            p_market * 100.0 if side == "YES" else (1.0 - p_market) * 100.0
        )
        out.append(
            {
                "forecast_id": int(row["id"]),
                "ticker": ticker,
                "side": side,
                "p_market": p_market,
                "p_model": float(row["p_model"]),
                "p_final": p_final,
                "edge": edge,
                "entry_price_cents": entry_cents,
                "verdict": verdict,
                "close_time": row["close_time"],
                "created_at": row["created_at"],
            }
        )
        remaining -= 1
    return out


def open_lot(
    conn: sqlite3.Connection,
    cand: dict,
    *,
    stake_cap: float,
    stake_pct: float,
    fee_mult: float,
) -> dict:
    balance = get_balance(conn)
    stake_target = min(stake_cap, balance * stake_pct)
    if stake_target < 1.0:
        raise RuntimeError(f"stake too small: balance={balance:.2f}")

    entry_dollars = cand["entry_price_cents"] / 100.0
    if entry_dollars <= 0 or entry_dollars >= 1.0:
        raise RuntimeError(f"bad entry {entry_dollars}")

    # qty such that cost + fee ≈ stake_target
    # cost = qty * entry; fee = ceil(fee_mult * entry * (1-entry) * qty * 100)/100
    # approximate qty = stake / entry, then recompute fee
    qty = stake_target / entry_dollars
    fee = order_fee(entry_dollars, qty, fee_mult)
    total = qty * entry_dollars + fee
    if total > balance:
        # scale down
        scale = (balance * 0.99) / total
        qty *= scale
        fee = order_fee(entry_dollars, qty, fee_mult)
        total = qty * entry_dollars + fee
    if total > balance or qty <= 0:
        raise RuntimeError("insufficient paper buying power")

    lot_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S+00:00")
    decision = {
        "forecast_id": cand["forecast_id"],
        "verdict": cand["verdict"],
        "p_market": cand["p_market"],
        "p_model": cand["p_model"],
        "p_final": cand["p_final"],
        "edge": cand["edge"],
        "fee_mult": fee_mult,
        "source": SOURCE,
    }
    conn.execute(
        """
        INSERT INTO paper_lots
            (id, ticker, title, category, side, entry_price_cents, qty,
             stake_dollars, source, decision_json, prediction_id, opened_at, status)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'Open')
        """,
        (
            lot_id,
            cand["ticker"],
            cand["ticker"],
            category_from_ticker(cand["ticker"]),
            cand["side"],
            cand["entry_price_cents"],
            qty,
            total,
            SOURCE,
            json.dumps(decision),
            str(cand["forecast_id"]),
            now,
        ),
    )
    conn.execute(
        """
        UPDATE paper_account
        SET balance_dollars = balance_dollars - ?, updated_at = ?
        WHERE id = 1
        """,
        (total, now),
    )
    return {
        "lot_id": lot_id,
        "qty": qty,
        "stake_dollars": total,
        "fee": fee,
        "opened_at": now,
        "balance_after": balance - total,
    }


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", type=Path, default=Path(os.environ.get("KALSHI_DB", DEFAULT_DB)))
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true", default=True)
    mode.add_argument("--execute", action="store_true")
    ap.add_argument("--limit", type=int, default=5)
    ap.add_argument("--min-edge", type=float, default=DEFAULT_MIN_EDGE)
    ap.add_argument("--fee-mult", type=float, default=DEFAULT_FEE_MULT)
    ap.add_argument("--stake-cap", type=float, default=DEFAULT_STAKE_CAP)
    ap.add_argument("--stake-pct", type=float, default=DEFAULT_STAKE_PCT)
    ap.add_argument("--daily-cap", type=int, default=DEFAULT_DAILY_CAP)
    args = ap.parse_args(argv)
    execute = bool(args.execute)

    if not args.db.exists():
        print(f"DB missing: {args.db}", file=sys.stderr)
        return 2

    conn = sqlite3.connect(str(args.db))
    try:
        ensure_paper_tables(conn)
        cands = build_candidates(
            conn,
            min_edge=args.min_edge,
            fee_mult=args.fee_mult,
            daily_cap=args.daily_cap,
            limit=args.limit,
        )
        results = []
        for c in cands:
            if execute:
                try:
                    opened = open_lot(
                        conn,
                        c,
                        stake_cap=args.stake_cap,
                        stake_pct=args.stake_pct,
                        fee_mult=args.fee_mult,
                    )
                    results.append({**c, **opened, "status": "opened"})
                except Exception as e:
                    results.append({**c, "status": "error", "error": str(e)})
            else:
                results.append({**c, "status": "dry_run"})
        if execute:
            conn.commit()
        else:
            conn.rollback()
    finally:
        bal = get_balance(conn) if args.db.exists() else None
        conn.close()

    print(
        json.dumps(
            {
                "mode": "execute" if execute else "dry_run",
                "candidates": len(cands),
                "balance": bal,
                "lots": results,
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
