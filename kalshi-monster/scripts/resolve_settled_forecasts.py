#!/usr/bin/env python3
"""Resolve open forecast ledger rows from the public Kalshi API.

Ops companion to the in-app auto-grade poller. Use when the desktop app is
not running so short-dated settles still advance the Phase 3 calibration gate
(n → 200 resolved forecasts). Never fabricates outcomes — only writes when
the public market has status finalized/determined/settled or a Yes/No result.

Usage (from anywhere):
  python kalshi-monster/scripts/resolve_settled_forecasts.py
  python kalshi-monster/scripts/resolve_settled_forecasts.py --dry-run

DB default: ~/.openclaw/kalshi-monster/predictions.db
"""
from __future__ import annotations

import argparse
import json
import sqlite3
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

API_BASE = "https://api.elections.kalshi.com/trade-api/v2"
UA = "kalshi-monster-cron/resolve-settled-forecasts/1.0"


def default_db() -> Path:
    return Path.home() / ".openclaw" / "kalshi-monster" / "predictions.db"


def fetch_market(ticker: str) -> dict:
    url = f"{API_BASE}/markets/{urllib.request.quote(ticker, safe='')}"
    req = urllib.request.Request(
        url, headers={"Accept": "application/json", "User-Agent": UA}
    )
    with urllib.request.urlopen(req, timeout=25) as resp:
        data = json.loads(resp.read().decode())
    return data.get("market") or data


def normalize_result(raw: str | None) -> str | None:
    if not raw:
        return None
    s = str(raw).strip().lower()
    if s in ("yes", "y"):
        return "Yes"
    if s in ("no", "n"):
        return "No"
    return None


def is_settled(market: dict) -> str | None:
    """Return Yes/No if market is settled; else None."""
    result = normalize_result(market.get("result"))
    if result:
        return result
    status = (market.get("status") or "").strip().lower()
    if status in ("finalized", "determined", "settled"):
        # result empty but terminal — cannot invent side
        return None
    return None


def brier(p: float, outcome: int) -> float:
    return (float(p) - float(outcome)) ** 2


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--db",
        type=Path,
        default=default_db(),
        help="Path to predictions.db",
    )
    ap.add_argument(
        "--dry-run",
        action="store_true",
        help="Probe API and print actions without writing",
    )
    args = ap.parse_args()

    if not args.db.exists():
        print(f"ERROR: DB not found: {args.db}", file=sys.stderr)
        return 2

    con = sqlite3.connect(str(args.db))
    con.row_factory = sqlite3.Row
    cur = con.cursor()

    unres = cur.execute(
        """
        SELECT id, market_ticker, p_market, p_model, p_final
        FROM forecasts
        WHERE outcome IS NULL
        ORDER BY id
        """
    ).fetchall()
    if not unres:
        print("No unresolved forecasts.")
        print_summary(cur)
        return 0

    by_ticker: dict[str, list] = {}
    for r in unres:
        by_ticker.setdefault(r["market_ticker"], []).append(r)

    print(f"Unresolved rows: {len(unres)} across {len(by_ticker)} tickers")
    resolved_at = (
        datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"
    )
    updated: list[dict] = []
    skipped_active = 0
    skipped_unknown = 0
    errors = 0

    for ticker, rows in sorted(by_ticker.items()):
        try:
            market = fetch_market(ticker)
        except Exception as e:
            print(f"  ERR  {ticker}: {e}")
            errors += 1
            continue

        actual = is_settled(market)
        status = market.get("status")
        if actual is None:
            result_raw = market.get("result") or ""
            st = (status or "").lower()
            if st in ("finalized", "determined", "settled") and not result_raw:
                print(
                    f"  SKIP {ticker}: status={status} but empty result "
                    f"({len(rows)} rows)"
                )
                skipped_unknown += 1
            else:
                print(
                    f"  OPEN {ticker}: status={status} result={result_raw!r} "
                    f"({len(rows)} rows)"
                )
                skipped_active += 1
            continue

        outcome = 1 if actual == "Yes" else 0
        print(
            f"  FINAL {ticker}: result={actual} → outcome={outcome} "
            f"({len(rows)} rows){' [dry-run]' if args.dry_run else ''}"
        )

        for r in rows:
            pid = r["id"]
            pm, pmod, pf = r["p_market"], r["p_model"], r["p_final"]
            bm = brier(pm, outcome)
            bf = brier(pf, outcome)
            bmod = brier(pmod, outcome) if pmod is not None else None
            if not args.dry_run:
                cur.execute(
                    """
                    UPDATE forecasts
                    SET resolved_at = ?,
                        outcome = ?,
                        brier_market = ?,
                        brier_model = ?,
                        brier_final = ?
                    WHERE id = ? AND outcome IS NULL
                    """,
                    (resolved_at, outcome, bm, bmod, bf, pid),
                )
            updated.append(
                {
                    "id": pid,
                    "ticker": ticker,
                    "actual": actual,
                    "brier_final": bf,
                    "brier_market": bm,
                }
            )

    if updated and not args.dry_run:
        con.commit()

    print()
    print(
        f"Resolved {len(updated)} row(s)"
        f"{' (dry-run, not written)' if args.dry_run else ''} "
        f"at {resolved_at}"
    )
    for u in updated:
        print(
            f"  id={u['id']} {u['ticker']} -> {u['actual']} "
            f"brier_f={u['brier_final']:.4f} brier_m={u['brier_market']:.4f}"
        )
    print(
        f"Skipped active={skipped_active} unknown_result={skipped_unknown} "
        f"api_errors={errors}"
    )
    print_summary(cur)
    con.close()
    return 0 if errors == 0 else 1


def print_summary(cur: sqlite3.Cursor) -> None:
    stats = cur.execute(
        """
        SELECT
          (SELECT COUNT(*) FROM forecasts) AS total,
          (SELECT COUNT(*) FROM forecasts WHERE outcome IS NOT NULL) AS resolved,
          (SELECT COUNT(*) FROM forecasts WHERE outcome IS NULL) AS unresolved,
          (SELECT AVG(brier_final) FROM forecasts WHERE outcome IS NOT NULL) AS bf,
          (SELECT AVG(brier_market) FROM forecasts WHERE outcome IS NOT NULL) AS bm,
          (SELECT AVG(brier_model) FROM forecasts
             WHERE outcome IS NOT NULL AND brier_model IS NOT NULL) AS bmod
        """
    ).fetchone()
    total, resolved, unresolved, bf, bm, bmod = stats
    print()
    print("Calibration summary:")
    print(f"  total={total} resolved={resolved} unresolved={unresolved}")
    if bf is not None and bm is not None:
        print(
            f"  mean Brier p_final={bf:.4f} p_market={bm:.4f} "
            f"p_model={bmod if bmod is None else f'{bmod:.4f}'}"
        )
        beat = bf < bm
        print(
            f"  p_final beats market: {beat} "
            f"(delta market-final={bm - bf:+.4f}; lower Brier is better)"
        )
    gate_n = 200
    print(
        f"  gate progress: {resolved}/{gate_n} "
        f"({100.0 * resolved / gate_n:.1f}%) — "
        f"{'OPEN candidate' if resolved >= gate_n and bf is not None and bm is not None and bf <= bm else 'LOCKED'}"
    )


if __name__ == "__main__":
    raise SystemExit(main())
