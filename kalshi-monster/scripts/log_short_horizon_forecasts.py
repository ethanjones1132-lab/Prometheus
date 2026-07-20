#!/usr/bin/env python3
"""Log market-only forecast rows on short-horizon Kalshi markets.

Ops companion for Phase 3 calibration sample-building (n → 200 resolved).
Matches the Rust pipeline honesty path when no agent opines:

  p_model = NULL
  p_final = p_market  (mid quote, clamped)
  verdict = pass
  reasons = market-only sample-build note

Never fabricates model edge. Prefer short close_time so the resolve script /
in-app poller can score outcomes quickly.

Usage:
  python kalshi-monster/scripts/log_short_horizon_forecasts.py --dry-run
  python kalshi-monster/scripts/log_short_horizon_forecasts.py --limit 80
  python kalshi-monster/scripts/log_short_horizon_forecasts.py --max-days 3 --limit 100

DB default: ~/.openclaw/kalshi-monster/predictions.db
"""
from __future__ import annotations

import argparse
import json
import sqlite3
import sys
import time
import urllib.error
import urllib.request
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlencode

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
from kalshi_ticker import (  # noqa: E402
    ensure_provenance_columns,
    eligible_resolved_rows,
    provenance_for,
)

API_BASE = "https://api.elections.kalshi.com/trade-api/v2"
UA = "kalshi-monster-cron/log-short-horizon-forecasts/1.0"

# Every row this script writes has p_final = p_market by construction, so it
# can never distinguish model from market. Recording that in `source` is what
# lets the gate exclude these rows structurally instead of counting them as
# model evidence. They remain in the ledger as honest data about *market*
# calibration.
SOURCE = "log_short_horizon_forecasts.py"

# Mirrors GateConfig::default().min_resolved in edge_engine/calibration.rs.
MIN_ELIGIBLE_FOR_GATE = 200

# Single-leg series that typically resolve in hours–days (not multi-year politics).
DEFAULT_SERIES = [
    "KXMLBGAME",
    "KXMLBTOTAL",
    "KXMLBTEAMTOTAL",
    "KXETH",
    "KXBTC",
    "KXHIGHNY",
    "KXHIGHCHI",
    "KXHIGHLAX",
    "KXHIGHMIA",
    "KXHIGHSF",
    "KXHIGHDEN",
    "KXHIGHATL",
    "KXHIGHPHIL",
    "KXINX",
    "KXFED",
]

# Skip multi-leg / parlay-style tickers even if they appear under a series.
SKIP_SUBSTR = ("MVE", "MULTIGAME", "CROSSCATEGORY", "PARLAY", "COMBO")


def default_db() -> Path:
    return Path.home() / ".openclaw" / "kalshi-monster" / "predictions.db"


def now_utc() -> datetime:
    return datetime.now(timezone.utc)


def iso_now() -> str:
    return now_utc().strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def fetch_json(path: str, params: dict | None = None) -> dict:
    url = API_BASE + path
    if params:
        url += "?" + urlencode(params)
    req = urllib.request.Request(
        url, headers={"Accept": "application/json", "User-Agent": UA}
    )
    last_err: Exception | None = None
    for attempt in range(5):
        try:
            with urllib.request.urlopen(req, timeout=40) as resp:
                return json.loads(resp.read().decode())
        except urllib.error.HTTPError as e:
            last_err = e
            if e.code == 429:
                time.sleep(min(30, 1.5 * (2**attempt)))
                continue
            raise
        except Exception as e:
            last_err = e
            time.sleep(0.5 * (attempt + 1))
    raise RuntimeError(f"fetch failed {path}: {last_err}")


def to_prob(raw) -> float | None:
    if raw is None or raw == "":
        return None
    try:
        f = float(raw)
    except (TypeError, ValueError):
        return None
    # Kalshi sometimes returns cents (0–100) or dollars (0–1).
    if f > 1.0:
        f /= 100.0
    return f


def market_mid(m: dict) -> tuple[float, float, float] | None:
    """Return (bid, ask, mid) in probability space, or None if unusable."""
    bid = to_prob(m.get("yes_bid_dollars") if m.get("yes_bid_dollars") not in (None, "") else m.get("yes_bid"))
    ask = to_prob(m.get("yes_ask_dollars") if m.get("yes_ask_dollars") not in (None, "") else m.get("yes_ask"))
    last = to_prob(
        m.get("last_price_dollars")
        if m.get("last_price_dollars") not in (None, "")
        else m.get("last_price")
    )
    if bid is None:
        bid = last
    if ask is None:
        ask = last
    if bid is None or ask is None:
        return None
    if ask < bid:
        bid, ask = ask, bid
    spread = ask - bid
    mid = (bid + ask) / 2.0
    if spread > 0.40 and last is not None and 0.02 < last < 0.98:
        mid = last
    elif spread > 0.40:
        return None
    mid = max(0.01, min(0.99, mid))
    # Extremes add little Brier signal and often reflect empty books.
    if mid <= 0.04 or mid >= 0.96:
        return None
    return bid, ask, mid


def parse_close(m: dict) -> tuple[datetime, float] | None:
    ct = m.get("close_time") or m.get("expected_expiration_time") or ""
    if not ct:
        return None
    try:
        dt = datetime.fromisoformat(str(ct).replace("Z", "+00:00"))
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
    except ValueError:
        return None
    days = (dt - now_utc()).total_seconds() / 86400.0
    return dt, days


def series_key(ticker: str) -> str:
    return ticker.split("-", 1)[0] if ticker else "UNKNOWN"


def collect_candidates(
    series_list: list[str],
    max_days: float,
    min_days: float,
) -> list[dict]:
    out: list[dict] = []
    seen: set[str] = set()
    for series in series_list:
        try:
            data = fetch_json(
                "/markets",
                {"limit": 200, "status": "open", "series_ticker": series},
            )
        except Exception as e:
            print(f"  WARN series {series}: {e}")
            time.sleep(0.5)
            continue
        markets = data.get("markets") or []
        print(f"  series {series}: {len(markets)} open")
        for m in markets:
            ticker = m.get("ticker") or ""
            if not ticker or ticker in seen:
                continue
            if any(s in ticker for s in SKIP_SUBSTR):
                continue
            parsed = parse_close(m)
            if parsed is None:
                continue
            _dt, days = parsed
            if not (min_days < days <= max_days):
                continue
            quote = market_mid(m)
            if quote is None:
                continue
            bid, ask, mid = quote
            seen.add(ticker)
            out.append(
                {
                    "ticker": ticker,
                    "title": (m.get("title") or "")[:120],
                    "close_time": m.get("close_time") or "",
                    "days": days,
                    "bid": bid,
                    "ask": ask,
                    "mid": mid,
                    "series": series_key(ticker),
                    "category": m.get("category") or series,
                }
            )
        time.sleep(0.35)
    return out


def diversify(candidates: list[dict], limit: int, max_per_series: int) -> list[dict]:
    """Prefer soonest close; cap per series so BTC buckets don't crowd out weather/MLB."""
    ordered = sorted(candidates, key=lambda c: (c["days"], abs(c["mid"] - 0.5)))
    picked: list[dict] = []
    per: dict[str, int] = defaultdict(int)
    for c in ordered:
        if len(picked) >= limit:
            break
        sk = c["series"]
        if per[sk] >= max_per_series:
            continue
        picked.append(c)
        per[sk] += 1
    # If still under limit, relax series cap once.
    if len(picked) < limit:
        already = {p["ticker"] for p in picked}
        for c in ordered:
            if len(picked) >= limit:
                break
            if c["ticker"] in already:
                continue
            picked.append(c)
            already.add(c["ticker"])
    return picked


def print_summary(cur: sqlite3.Cursor) -> None:
    """Report the raw ledger and the sample the gate actually tests.

    This script's own rows are market-only: `p_model` is NULL and
    `p_final = p_market`. They move `total`/`resolved` but contribute nothing
    to `eligible`, and reporting only the former is what made a ledger of 213
    market-echo rows look like it had cleared an n>=200 model-skill gate.
    """
    stats = cur.execute(
        """
        SELECT
          (SELECT COUNT(*) FROM forecasts) AS total,
          (SELECT COUNT(*) FROM forecasts WHERE outcome IS NOT NULL) AS resolved,
          (SELECT COUNT(*) FROM forecasts WHERE outcome IS NULL) AS unresolved,
          (SELECT AVG(brier_final) FROM forecasts WHERE outcome IS NOT NULL) AS bf,
          (SELECT AVG(brier_market) FROM forecasts WHERE outcome IS NOT NULL) AS bm
        """
    ).fetchone()
    total, resolved, unresolved, bf, bm = stats
    eligible = eligible_resolved_rows(cur)
    n_elig = len(eligible)
    print()
    print("Calibration summary:")
    print(f"  total={total} resolved={resolved} unresolved={unresolved}")
    if bf is not None and bm is not None:
        print(f"  [raw] mean Brier p_final={bf:.4f} p_market={bm:.4f}")
        print(
            f"  [raw] p_final beats market: {bf < bm} "
            f"(delta market-final={bm - bf:+.4f}) "
            f"— market-only rows make this comparison trivially equal"
        )
    print(
        f"  eligible={n_elig} (p_model present, pre-event, one row per event) "
        f"— rows this script writes are market-only and never eligible"
    )
    if n_elig:
        ebf = sum((r[2] - r[3]) ** 2 for r in eligible) / n_elig
        ebm = sum((r[0] - r[3]) ** 2 for r in eligible) / n_elig
        brier_ok = ebf <= ebm
        print(f"  [eligible] Brier p_final={ebf:.4f} p_market={ebm:.4f}")
    else:
        brier_ok = False
    open_candidate = n_elig >= MIN_ELIGIBLE_FOR_GATE and brier_ok
    print(
        f"  gate progress: {n_elig}/{MIN_ELIGIBLE_FOR_GATE} eligible "
        f"({100.0 * n_elig / MIN_ELIGIBLE_FOR_GATE:.1f}%) — "
        f"{'OPEN candidate' if open_candidate else 'LOCKED'}"
    )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", type=Path, default=default_db())
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--limit", type=int, default=80, help="Max new forecast rows")
    ap.add_argument("--max-days", type=float, default=3.0)
    ap.add_argument("--min-days", type=float, default=0.0)
    ap.add_argument(
        "--max-per-series",
        type=int,
        default=12,
        help="Cap rows per ticker prefix for diversity",
    )
    ap.add_argument(
        "--series",
        nargs="*",
        default=None,
        help="Override series_ticker list",
    )
    ap.add_argument(
        "--allow-duplicate-ticker",
        action="store_true",
        help="Log even if an unresolved forecast already exists for ticker",
    )
    args = ap.parse_args()

    if not args.db.exists():
        print(f"ERROR: DB not found: {args.db}", file=sys.stderr)
        return 2

    series_list = args.series or DEFAULT_SERIES
    print(
        f"Scanning {len(series_list)} series for close in "
        f"({args.min_days}, {args.max_days}] days …"
    )
    candidates = collect_candidates(series_list, args.max_days, args.min_days)
    print(f"Candidates after quote/horizon filter: {len(candidates)}")

    con = sqlite3.connect(str(args.db))
    con.row_factory = sqlite3.Row
    cur = con.cursor()
    # Safe if the app has already migrated; needed if it has not.
    ensure_provenance_columns(con)

    existing_unresolved = {
        r[0]
        for r in cur.execute(
            "SELECT DISTINCT market_ticker FROM forecasts WHERE outcome IS NULL"
        ).fetchall()
    }
    # Also skip tickers logged in the last 12h to avoid spam if re-run.
    recent = {
        r[0]
        for r in cur.execute(
            """
            SELECT DISTINCT market_ticker FROM forecasts
            WHERE created_at >= datetime('now', '-12 hours')
            """
        ).fetchall()
    }

    filtered = []
    skipped_dup = 0
    for c in candidates:
        t = c["ticker"]
        if not args.allow_duplicate_ticker and t in existing_unresolved:
            skipped_dup += 1
            continue
        if t in recent:
            skipped_dup += 1
            continue
        filtered.append(c)

    picked = diversify(filtered, args.limit, args.max_per_series)
    print(
        f"Selected {len(picked)} (skipped_dup_or_recent={skipped_dup}, "
        f"limit={args.limit}, max_per_series={args.max_per_series})"
    )

    reasons = json.dumps(
        [
            "market-only sample-build; p_final=p_market (no fabricated model edge)",
            "source:log_short_horizon_forecasts.py",
            "depth:quick",
        ]
    )
    created_at = iso_now()
    inserted: list[dict] = []

    for c in picked:
        p_market = float(c["mid"])
        p_final = p_market
        print(
            f"  LOG  {c['days']:.2f}d mid={p_market:.3f} {c['ticker']}"
            f"{' [dry-run]' if args.dry_run else ''}"
        )
        if not args.dry_run:
            event_key, event_start_at, in_play = provenance_for(
                c["ticker"], created_at
            )
            cur.execute(
                """
                INSERT INTO forecasts (
                    market_ticker, created_at, close_time,
                    p_market, p_model, p_final,
                    verdict, verdict_reasons,
                    stake_suggested, agent_breakdown,
                    event_start_at, is_in_play, source, event_key, agents_opining
                ) VALUES (?, ?, ?, ?, NULL, ?, 'pass', ?, NULL, NULL, ?, ?, ?, ?, 0)
                """,
                (
                    c["ticker"],
                    created_at,
                    c["close_time"],
                    p_market,
                    p_final,
                    reasons,
                    event_start_at,
                    in_play,
                    SOURCE,
                    event_key,
                ),
            )
            fid = cur.lastrowid
        else:
            fid = None
        inserted.append(
            {
                "id": fid,
                "ticker": c["ticker"],
                "days": c["days"],
                "p_market": p_market,
                "title": c["title"],
            }
        )

    if inserted and not args.dry_run:
        con.commit()

    print()
    print(
        f"Logged {len(inserted)} forecast row(s)"
        f"{' (dry-run, not written)' if args.dry_run else ''} at {created_at}"
    )
    by_series: dict[str, int] = defaultdict(int)
    for row in inserted:
        by_series[series_key(row["ticker"])] += 1
    print("By series:", dict(sorted(by_series.items(), key=lambda kv: -kv[1])))
    print_summary(cur)
    con.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
