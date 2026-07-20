#!/usr/bin/env python3
"""Resolve open forecast ledger rows from the public Kalshi API.

Ops companion to the in-app auto-grade poller. Use when the desktop app is
not running so short-dated settles still advance the Phase 3 calibration gate
(n → 200 resolved forecasts). Never fabricates outcomes — only writes when
the public market has status finalized/determined/settled or a Yes/No result.

Usage (from anywhere):
  python kalshi-monster/scripts/resolve_settled_forecasts.py
  python kalshi-monster/scripts/resolve_settled_forecasts.py --dry-run
  python kalshi-monster/scripts/resolve_settled_forecasts.py --poll-minutes 45 --poll-interval 90

DB default: ~/.openclaw/kalshi-monster/predictions.db
"""
from __future__ import annotations

import argparse
import sqlite3
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

# Shared ticker / eligibility helpers (same rules as Rust gate).
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
try:
    from kalshi_ticker import (  # noqa: E402
        eligible_resolved_rows,
        ensure_provenance_columns,
    )
except ImportError:  # pragma: no cover
    eligible_resolved_rows = None  # type: ignore[assignment]
    ensure_provenance_columns = None  # type: ignore[assignment]

API_BASE = "https://api.elections.kalshi.com/trade-api/v2"
UA = "kalshi-monster-cron/resolve-settled-forecasts/1.2"


def default_db() -> Path:
    return Path.home() / ".openclaw" / "kalshi-monster" / "predictions.db"


def fetch_market(ticker: str) -> dict:
    url = f"{API_BASE}/markets/{urllib.request.quote(ticker, safe='')}"
    req = urllib.request.Request(
        url, headers={"Accept": "application/json", "User-Agent": UA}
    )
    with urllib.request.urlopen(req, timeout=25) as resp:
        data = json_loads(resp.read().decode())
    return data.get("market") or data


def json_loads(s: str):
    import json

    return json.loads(s)


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


def iso_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def run_once(db: Path, dry_run: bool, quiet_open: bool = False) -> dict:
    """One resolve pass. Returns stats dict including 'updated' count."""
    con = sqlite3.connect(str(db))
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
        con.close()
        return {
            "updated": 0,
            "skipped_active": 0,
            "skipped_unknown": 0,
            "errors": 0,
            "unresolved_start": 0,
        }

    by_ticker: dict[str, list] = {}
    for r in unres:
        by_ticker.setdefault(r["market_ticker"], []).append(r)

    print(f"Unresolved rows: {len(unres)} across {len(by_ticker)} tickers")
    resolved_at = iso_now()
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
                if not quiet_open:
                    print(
                        f"  OPEN {ticker}: status={status} result={result_raw!r} "
                        f"({len(rows)} rows)"
                    )
                skipped_active += 1
            continue

        outcome = 1 if actual == "Yes" else 0
        print(
            f"  FINAL {ticker}: result={actual} → outcome={outcome} "
            f"({len(rows)} rows){' [dry-run]' if dry_run else ''}"
        )

        for r in rows:
            pid = r["id"]
            pm, pmod, pf = r["p_market"], r["p_model"], r["p_final"]
            bm = brier(pm, outcome)
            bf = brier(pf, outcome)
            bmod = brier(pmod, outcome) if pmod is not None else None
            if not dry_run:
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

    if updated and not dry_run:
        con.commit()

    print()
    print(
        f"Resolved {len(updated)} row(s)"
        f"{' (dry-run, not written)' if dry_run else ''} "
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
    return {
        "updated": len(updated),
        "skipped_active": skipped_active,
        "skipped_unknown": skipped_unknown,
        "errors": errors,
        "unresolved_start": len(unres),
    }


def main() -> int:
    try:
        sys.stdout.reconfigure(line_buffering=True)  # type: ignore[attr-defined]
    except Exception:
        pass
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
    ap.add_argument(
        "--poll-minutes",
        type=float,
        default=0.0,
        help=(
            "If >0, keep re-probing for this many minutes when the first pass "
            "finds zero new settles (useful near hourly crypto close times). "
            "First pass always runs immediately."
        ),
    )
    ap.add_argument(
        "--poll-interval",
        type=float,
        default=90.0,
        help="Seconds between poll retries (default 90)",
    )
    ap.add_argument(
        "--quiet-open",
        action="store_true",
        help="Suppress per-ticker OPEN lines (summary still prints)",
    )
    args = ap.parse_args()

    if not args.db.exists():
        print(f"ERROR: DB not found: {args.db}", file=sys.stderr)
        return 2

    deadline = None
    if args.poll_minutes and args.poll_minutes > 0:
        deadline = time.time() + (args.poll_minutes * 60.0)
        print(
            f"Poll mode: up to {args.poll_minutes:g} min, "
            f"interval={args.poll_interval:g}s"
        )

    attempt = 0
    total_updated = 0
    last_errors = 0
    while True:
        attempt += 1
        if attempt > 1:
            print(f"\n--- poll attempt {attempt} at {iso_now()} ---")
        stats = run_once(args.db, args.dry_run, quiet_open=args.quiet_open)
        total_updated += int(stats["updated"])
        last_errors = int(stats["errors"])

        if deadline is None:
            break
        # Stop early once we got at least one settle this session, OR time up
        if stats["updated"] > 0:
            print(
                f"\nPoll complete: +{stats['updated']} this pass "
                f"(session total +{total_updated})"
            )
            break
        remaining = deadline - time.time()
        if remaining <= 0:
            print("\nPoll window exhausted with 0 new settles this session.")
            break
        sleep_for = min(float(args.poll_interval), max(1.0, remaining))
        print(
            f"No new settles; sleeping {sleep_for:.0f}s "
            f"({remaining:.0f}s left in poll window)…"
        )
        time.sleep(sleep_for)

    if attempt > 1:
        print(f"\nSession resolved total: {total_updated} row(s)")
    return 0 if last_errors == 0 else 1


def print_summary(cur: sqlite3.Cursor) -> None:
    """Report raw ledger counts and the *eligible* sample the gate tests.

    Raw resolved count is dominated by market-only sample-build rows
    (`p_final ≈ p_market`). The Phase 3 gate (Rust `evaluate_gate`) only
    counts model-bearing, pre-event, one-per-event rows — report that here
    so cron notes stop calling n≥200 raw an "OPEN candidate".
    """
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
            f"  [raw] mean Brier p_final={bf:.4f} p_market={bm:.4f} "
            f"p_model={bmod if bmod is None else f'{bmod:.4f}'}"
        )
        beat = bf < bm
        print(
            f"  [raw] p_final beats market: {beat} "
            f"(delta market-final={bm - bf:+.4f}) "
            f"— market-only rows make this nearly equal by construction"
        )
    gate_n = 200
    n_elig = 0
    brier_ok = False
    if eligible_resolved_rows is not None:
        conn = cur.connection
        if ensure_provenance_columns is not None:
            ensure_provenance_columns(conn)
        eligible = eligible_resolved_rows(conn)
        n_elig = len(eligible)
        if n_elig:
            ebf = sum((r[2] - r[3]) ** 2 for r in eligible) / n_elig
            ebm = sum((r[0] - r[3]) ** 2 for r in eligible) / n_elig
            brier_ok = ebf <= ebm
            print(
                f"  [eligible] n={n_elig}  Brier p_final={ebf:.4f} "
                f"p_market={ebm:.4f}"
            )
        else:
            print(
                "  [eligible] n=0 (need p_model + pre-event + one row per event)"
            )
    else:
        print("  [eligible] helper unavailable — install scripts/kalshi_ticker.py")
    open_candidate = n_elig >= gate_n and brier_ok
    print(
        f"  gate progress: {n_elig}/{gate_n} eligible "
        f"({100.0 * n_elig / gate_n:.1f}%) — "
        f"{'OPEN candidate' if open_candidate else 'LOCKED'} "
        f"(raw resolved={resolved}; paper P&L leg separate)"
    )


if __name__ == "__main__":
    raise SystemExit(main())
