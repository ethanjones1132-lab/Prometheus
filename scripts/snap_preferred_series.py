#!/usr/bin/env python3
"""Cron-friendly preferred-series price snapshots into predictions.db.

The Tauri app's price_tracker polls preferred series when running. This script
covers the common case where the app is closed but resolve/pipeline crons still
need real `kalshi_price_snapshots` for contract_tape.

Writes into ~/.openclaw/kalshi-monster/predictions.db (override via KALSHI_DB).
"""
from __future__ import annotations

import argparse
import json
import os
import sqlite3
import sys
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlencode

KALSHI_MARKETS = "https://api.elections.kalshi.com/trade-api/v2/markets"
PREFERRED_SERIES = (
    "KXBTC",
    "KXETH",
    "KXINX",
    "KXNASDAQ100",
    "KXNDX",
    "KXGOLD",
    "KXWTI",
    "KXAAPL",
    "KXTSLA",
    "KXNVDA",
)
DEFAULT_DB = (
    Path(os.environ.get("USERPROFILE", os.environ.get("HOME", ".")))
    / ".openclaw/kalshi-monster/predictions.db"
)

DDL = """
CREATE TABLE IF NOT EXISTS kalshi_price_snapshots (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    category TEXT NOT NULL DEFAULT '',
    yes_prob_pct REAL NOT NULL,
    yes_bid REAL NOT NULL,
    yes_ask REAL NOT NULL,
    spread REAL NOT NULL,
    volume_24h REAL NOT NULL DEFAULT 0,
    liquidity REAL NOT NULL DEFAULT 0,
    snapshot_at TEXT NOT NULL
)
"""


def http_json(url: str) -> dict:
    req = urllib.request.Request(
        url, headers={"User-Agent": "kalshi-monster-snap-preferred/0.1"}
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode())


def _f(m: dict, *keys: str, default: float = 0.0) -> float:
    for k in keys:
        v = m.get(k)
        if v is None:
            continue
        try:
            return float(v)
        except (TypeError, ValueError):
            continue
    return default


def market_mid_pct(m: dict) -> float:
    """YES mid in 0–100 (matches Rust KalshiMarketSummary.yes_prob_pct)."""
    bid = _f(m, "yes_bid_dollars", "yes_bid")
    ask = _f(m, "yes_ask_dollars", "yes_ask")
    # API may return dollars (0–1) or cents (0–100).
    if bid > 1.0 or ask > 1.0:
        bid, ask = bid / 100.0, ask / 100.0
    if bid > 0 and ask > 0:
        mid = (bid + ask) / 2.0
    else:
        last = _f(m, "last_price_dollars", "last_price", "yes_price")
        if last > 1.0:
            last /= 100.0
        mid = last if last > 0 else 0.5
    return max(0.0, min(100.0, mid * 100.0))


def ensure_tables(conn: sqlite3.Connection) -> None:
    conn.execute(DDL)
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_kps_ticker ON kalshi_price_snapshots(ticker)"
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_kps_snapshot ON kalshi_price_snapshots(snapshot_at)"
    )
    conn.commit()


def fetch_series_markets(series: str, limit: int = 100) -> list[dict]:
    params = urlencode(
        {
            "limit": limit,
            "status": "open",
            "series_ticker": series,
            "mve_filter": "exclude",
        }
    )
    data = http_json(f"{KALSHI_MARKETS}?{params}")
    return list(data.get("markets") or [])


def snapshot_once(conn: sqlite3.Connection, limit_per_series: int = 100) -> dict:
    ensure_tables(conn)
    snapshot_at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S+00:00")
    inserted = 0
    seen = 0
    by_series: dict[str, int] = {}
    for series in PREFERRED_SERIES:
        try:
            markets = fetch_series_markets(series, limit_per_series)
        except Exception as e:
            print(f"  WARN {series}: {e}", file=sys.stderr)
            by_series[series] = -1
            continue
        by_series[series] = len(markets)
        for m in markets:
            ticker = (m.get("ticker") or "").strip()
            if not ticker:
                continue
            seen += 1
            bid = _f(m, "yes_bid_dollars", "yes_bid")
            ask = _f(m, "yes_ask_dollars", "yes_ask")
            if bid > 1.0 or ask > 1.0:
                bid, ask = bid / 100.0, ask / 100.0
            # store bid/ask as 0–100 like app summaries often do; tolerate either
            bid_pct = bid * 100.0 if bid <= 1.0 else bid
            ask_pct = ask * 100.0 if ask <= 1.0 else ask
            mid_pct = market_mid_pct(m)
            spread = max(0.0, ask_pct - bid_pct)
            vol = _f(m, "volume_24h", "volume")
            liq = _f(m, "liquidity_dollars", "liquidity")
            title = (m.get("title") or m.get("subtitle") or "")[:200]
            cat = (m.get("category") or series)[:64]
            sid = f"{ticker}-{snapshot_at}"
            cur = conn.execute(
                """
                INSERT OR IGNORE INTO kalshi_price_snapshots
                    (id, ticker, title, category, yes_prob_pct, yes_bid, yes_ask,
                     spread, volume_24h, liquidity, snapshot_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    sid,
                    ticker,
                    title,
                    cat,
                    mid_pct,
                    bid_pct,
                    ask_pct,
                    spread,
                    vol,
                    liq,
                    snapshot_at,
                ),
            )
            inserted += cur.rowcount
    conn.commit()
    return {
        "snapshot_at": snapshot_at,
        "markets_seen": seen,
        "snapshots_taken": inserted,
        "by_series": by_series,
    }


def prune(conn: sqlite3.Connection, keep_days: int = 14) -> int:
    cur = conn.execute(
        """
        DELETE FROM kalshi_price_snapshots
        WHERE snapshot_at < datetime('now', ?)
        """,
        (f"-{int(keep_days)} days",),
    )
    conn.commit()
    return cur.rowcount


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", type=Path, default=Path(os.environ.get("KALSHI_DB", DEFAULT_DB)))
    ap.add_argument("--limit", type=int, default=100, help="markets per series")
    ap.add_argument("--prune-days", type=int, default=14)
    ap.add_argument("--no-prune", action="store_true")
    args = ap.parse_args(argv)

    args.db.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(args.db))
    try:
        result = snapshot_once(conn, args.limit)
        pruned = 0 if args.no_prune else prune(conn, args.prune_days)
    finally:
        conn.close()

    print(
        json.dumps(
            {**result, "pruned": pruned, "db": str(args.db)},
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
