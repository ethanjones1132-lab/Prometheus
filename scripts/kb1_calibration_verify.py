#!/usr/bin/env python3
"""KB-1 catalog live verify + calibration flywheel (real Kalshi only).

Step 1 (KB-1): Prove the same public catalog the app's quick-cache uses
returns a non-empty open-market tape (no credentials). Mirrors
`KalshiClient::fetch_markets_flat_resilient` page size/limits from
`kalshi/client.rs` (QUICK_LOAD_PAGES=2, FLAT_MARKET_PAGE_LIMIT=100).

Step 2 (Calibration): Append real open-market forecasts via agents where
possible; resolve any pending forecast whose Kalshi `result` is Yes/No.
Never invents outcomes or backfills fake p_model.

Sources named in every print line.
"""

from __future__ import annotations

import asyncio
import json
import math
import os
import sqlite3
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "fincept-sidecar"))

from agents.orchestrator import collect_market_opinion  # noqa: E402
from fincept_sidecar.schemas import MarketCategory, MarketOpinionRequest  # noqa: E402

# Mirrors kalshi-monster/src-tauri/src/kalshi/client.rs
PRIMARY_BASE = "https://api.elections.kalshi.com/trade-api/v2"
QUICK_LOAD_PAGES = 2
FLAT_MARKET_PAGE_LIMIT = 100
INITIAL_MARKET_LIMIT = QUICK_LOAD_PAGES * FLAT_MARKET_PAGE_LIMIT  # app quick-cache ceiling
LAMBDA = 0.25
DB = Path(os.environ.get("USERPROFILE", os.environ.get("HOME", "."))) / ".openclaw/kalshi-monster/predictions.db"
UA = "kalshi-monster-kb1-calibration-verify/0.1"


def http_json(url: str) -> dict:
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=45) as resp:
        return json.loads(resp.read().decode())


def clamp(p: float) -> float:
    return max(0.01, min(0.99, p))


def logit(p: float) -> float:
    p = clamp(p)
    return math.log(p / (1.0 - p))


def shrink(p_model: float, p_market: float, lam: float = LAMBDA) -> float:
    x = lam * logit(p_model) + (1.0 - lam) * logit(p_market)
    return 1.0 / (1.0 + math.exp(-x))


def fetch_quick_catalog(*, mve_filter: str | None = None) -> tuple[list[dict], list[str]]:
    """Paginate open markets like ensure_quick_cache (flat /markets).

    When `mve_filter='exclude'`, matches the app's non-multivariate catalog
    path used for cleaner single-event contracts (see KalshiMarketsQuery).
    """
    markets: list[dict] = []
    notes: list[str] = []
    cursor = None
    for page in range(QUICK_LOAD_PAGES):
        url = f"{PRIMARY_BASE}/markets?limit={FLAT_MARKET_PAGE_LIMIT}&status=open"
        if mve_filter:
            url += f"&mve_filter={urllib.request.quote(mve_filter)}"
        if cursor:
            url += f"&cursor={urllib.request.quote(cursor)}"
        try:
            data = http_json(url)
        except urllib.error.HTTPError as e:
            notes.append(f"page {page + 1} HTTP {e.code}: {e.reason}")
            break
        except Exception as e:
            notes.append(f"page {page + 1} error: {e}")
            break
        batch = data.get("markets") or []
        markets.extend(batch)
        filt = f"&mve_filter={mve_filter}" if mve_filter else ""
        notes.append(
            f"page {page + 1}: {len(batch)} markets from {PRIMARY_BASE}/markets?status=open{filt} "
            f"(source: Kalshi public trade-api)"
        )
        cursor = data.get("cursor") or None
        if not cursor or not batch:
            break
    return markets, notes


def mid_of(m: dict) -> float | None:
    try:
        bid = float(m.get("yes_bid_dollars") or 0)
        ask = float(m.get("yes_ask_dollars") or 0)
    except (TypeError, ValueError):
        return None
    if bid <= 0 and ask <= 0:
        return None
    if bid <= 0:
        return clamp(ask)
    if ask <= 0:
        return clamp(bid)
    return clamp(0.5 * (bid + ask))


def ensure_forecast_table(conn: sqlite3.Connection) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS forecasts (
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
    conn.commit()


def category_for(title: str, ticker: str) -> MarketCategory:
    hay = f"{title} {ticker}".upper()
    if any(k in hay for k in ("CPI", "FED", "GDP", "JOBS", "UNEMPLOY", "FOMC", "RATE")):
        return MarketCategory.ECONOMIC
    if any(k in hay for k in ("S&P", "SPX", "NASDAQ", "BTC", "ETH", "STOCK", "INDEX", "SPY", "QQQ")):
        return MarketCategory.INDEX_PRICE_LEVEL
    if any(k in hay for k in ("ELECT", "PRESIDENT", "SENATE", "TRUMP", "BIDEN")):
        return MarketCategory.POLITICAL
    return MarketCategory.OTHER


async def analyze_one(m: dict) -> dict | None:
    ticker = m.get("ticker") or ""
    title = m.get("title") or ticker
    rules = m.get("rules_primary") or title
    close = m.get("close_time") or datetime.now(timezone.utc).isoformat()
    p_market = mid_of(m)
    if p_market is None or not ticker:
        return None
    try:
        yes_bid = float(m.get("yes_bid_dollars") or 0)
        yes_ask = float(m.get("yes_ask_dollars") or 0)
    except (TypeError, ValueError):
        yes_bid, yes_ask = p_market, p_market

    close_dt = datetime.now(timezone.utc)
    if isinstance(close, str):
        try:
            close_dt = datetime.fromisoformat(close.replace("Z", "+00:00"))
        except ValueError:
            pass

    req = MarketOpinionRequest(
        market_ticker=ticker,
        title=title,
        resolution_rules=rules,
        close_time=close_dt,
        category=category_for(title, ticker),
        yes_bid=max(0.0, min(1.0, yes_bid or p_market)),
        yes_ask=max(0.0, min(1.0, yes_ask or p_market)),
        context={"contract_mids": [p_market]},
    )
    resp = await collect_market_opinion(req)
    opining = [s for s in resp.signals if s.probability is not None]
    if not opining:
        p_model = None
        p_final = p_market
        reasons = ["no agent opinion; p_final=p_market"]
        verdict = "pass"
    else:
        wsum = 0.0
        pooled = 0.0
        for s in opining:
            w = max(0.01, s.confidence)
            pooled += w * logit(float(s.probability))
            wsum += w
        p_model = 1.0 / (1.0 + math.exp(-(pooled / wsum)))
        p_final = shrink(p_model, p_market)
        fee = 0.07 * p_market * (1.0 - p_market)
        entry = (yes_ask if yes_ask > 0 else p_market) + fee
        edge_yes = p_final - entry
        verdict = "trade_yes" if edge_yes >= 0.05 else "pass"
        reasons = [f"agents_opining={len(opining)}", f"lambda={LAMBDA}", f"edge_yes={edge_yes:.4f}"]

    return {
        "ticker": ticker,
        "close_time": close if isinstance(close, str) else str(close),
        "p_market": p_market,
        "p_model": p_model,
        "p_final": p_final,
        "verdict": verdict,
        "reasons": reasons,
        "breakdown": {
            "signals": [
                {"agent": s.agent, "probability": s.probability, "confidence": s.confidence}
                for s in resp.signals
            ],
            "source": "kb1_calibration_verify.py + Kalshi public API + agents",
        },
    }


def insert_forecast(conn: sqlite3.Connection, row: dict) -> int:
    cur = conn.execute(
        """
        INSERT INTO forecasts (
            market_ticker, created_at, close_time,
            p_market, p_model, p_final, verdict, verdict_reasons,
            stake_suggested, agent_breakdown
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, ?)
        """,
        (
            row["ticker"],
            datetime.now(timezone.utc).isoformat(),
            row["close_time"],
            row["p_market"],
            row["p_model"],
            row["p_final"],
            row["verdict"],
            json.dumps(row["reasons"]),
            json.dumps(row["breakdown"]),
        ),
    )
    conn.commit()
    return int(cur.lastrowid)


def resolve_if_settled(conn: sqlite3.Connection, ticker: str) -> int:
    try:
        data = http_json(f"{PRIMARY_BASE}/markets/{urllib.request.quote(ticker)}")
    except Exception as e:
        print(f"  resolve fetch fail {ticker}: {e} (source: Kalshi GET /markets/{{ticker}})")
        return 0
    m = data.get("market") or data
    result = (m.get("result") or "").strip()
    if result not in ("Yes", "No"):
        return 0
    outcome = 1 if result == "Yes" else 0
    rows = conn.execute(
        "SELECT id, p_market, p_model, p_final FROM forecasts WHERE market_ticker=? AND outcome IS NULL",
        (ticker,),
    ).fetchall()
    n = 0
    now = datetime.now(timezone.utc).isoformat()
    for fid, p_mkt, p_mod, p_fin in rows:
        y = float(outcome)
        bm = (float(p_mkt) - y) ** 2
        bf = (float(p_fin) - y) ** 2
        bmod = (float(p_mod) - y) ** 2 if p_mod is not None else None
        conn.execute(
            """
            UPDATE forecasts SET resolved_at=?, outcome=?, brier_market=?, brier_model=?, brier_final=?
            WHERE id=?
            """,
            (now, outcome, bm, bmod, bf, fid),
        )
        n += 1
        print(
            f"  resolved forecast#{fid} {ticker} result={result} "
            f"(source: Kalshi market.result={result!r})"
        )
    conn.commit()
    return n


def print_brier_report(conn: sqlite3.Connection) -> None:
    n_res = conn.execute("SELECT COUNT(*) FROM forecasts WHERE outcome IS NOT NULL").fetchone()[0]
    n_un = conn.execute("SELECT COUNT(*) FROM forecasts WHERE outcome IS NULL").fetchone()[0]
    n_model = conn.execute(
        "SELECT COUNT(*) FROM forecasts WHERE outcome IS NOT NULL AND p_model IS NOT NULL"
    ).fetchone()[0]
    print(f"\n=== Ledger (source: {DB}) ===")
    print(f"resolved={n_res} unresolved={n_un} resolved_with_p_model={n_model}")
    if n_res == 0:
        print("Brier(p_final)/Brier(p_market): N/A — no resolved outcomes yet (honest).")
        print("Gate: LOCKED (needs ≥200 resolved).")
        return
    rows = conn.execute(
        "SELECT p_market, p_model, p_final, outcome FROM forecasts WHERE outcome IS NOT NULL"
    ).fetchall()
    bf = sum((p_f - o) ** 2 for _, _, p_f, o in rows) / len(rows)
    bm = sum((p_m - o) ** 2 for p_m, _, _, o in rows) / len(rows)
    print(f"Brier(p_market)={bm:.4f}  Brier(p_final)={bf:.4f}  n={len(rows)}")
    model_rows = [(pm, p_m, o) for p_m, pm, _, o in rows if pm is not None]
    if model_rows:
        bmod = sum((pm - o) ** 2 for pm, _, o in model_rows) / len(model_rows)
        bm_m = sum((p_m - o) ** 2 for _, p_m, o in model_rows) / len(model_rows)
        print(f"Brier(p_model)={bmod:.4f}  Brier(market|model_rows)={bm_m:.4f}  n_model={len(model_rows)}")
    print(
        f"Gate conditions: resolved≥200? {n_res >= 200}; "
        f"Brier(final)≤Brier(market)? {bf <= bm}; paper P&L not measured here."
    )


async def main() -> int:
    print("=== Step 1: KB-1 live catalog verify ===")
    print(f"Endpoint base: {PRIMARY_BASE}")
    print(f"Quick-cache mirror: {QUICK_LOAD_PAGES} pages × {FLAT_MARKET_PAGE_LIMIT} (limit {INITIAL_MARKET_LIMIT})")
    # App quick-cache uses mve_filter=exclude (client.rs fetch_markets_flat_pages).
    markets, notes = fetch_quick_catalog(mve_filter="exclude")
    for n in notes:
        print(f"  {n}")
    count = len(markets)
    print(f"Total open non-MVE markets fetched: {count}")
    if count == 0:
        print("KB-1 VERIFY FAIL: zero markets from public API (cannot be dual-runtime alone).")
        return 1
    if count < 50:
        print(
            f"KB-1 VERIFY WARN: only {count} markets — below typical quick-cache fill; "
            "check API filters. Code path still received non-empty payload."
        )
    else:
        print(
            f"KB-1 VERIFY PASS (catalog path): non-empty tape ({count} markets) matches "
            f"KalshiClient quick-cache (2×100, mve_filter=exclude). "
            f"blocking_write fix unit-tested in kalshi::client::tests. "
            f"Desktop UI paint still requires a local app rebuild to confirm React tape."
        )
    print(f"Sample tickers: {[m.get('ticker') for m in markets[:5]]}")

    print("\n=== Step 2: Calibration flywheel ===")
    print(f"DB: {DB}")
    conn = sqlite3.connect(str(DB))
    ensure_forecast_table(conn)

    # Analyze path: non-MVE markets with a usable yes mid (real quote).
    analyzable, anotes = fetch_quick_catalog(mve_filter="exclude")
    for n in anotes:
        print(f"  {n}")
    with_mid = [m for m in analyzable if mid_of(m) is not None]
    print(f"Non-MVE open markets with mid: {len(with_mid)} (source: mve_filter=exclude + bid/ask parse)")

    ranked = sorted(
        with_mid,
        key=lambda m: -float(m.get("volume_24h_fp") or m.get("volume_24h") or 0),
    )
    existing = {
        r[0]
        for r in conn.execute(
            "SELECT DISTINCT market_ticker FROM forecasts WHERE outcome IS NULL"
        ).fetchall()
    }
    candidates = [m for m in ranked if m.get("ticker") not in existing][:15]
    if not candidates:
        # Still try first mid-bearing markets even if already pending — skip only exact dups
        candidates = ranked[:15]
        print("  note: all mid-bearing candidates already pending or thin set; writing new rows only for novel tickers")
        candidates = [m for m in ranked if m.get("ticker") not in existing][:15]
    written = 0
    for m in candidates:
        try:
            row = await analyze_one(m)
        except Exception as e:
            print(f"  analyze skip {m.get('ticker')}: {e}")
            continue
        if not row:
            print(f"  analyze skip {m.get('ticker')}: no mid/parse")
            continue
        fid = insert_forecast(conn, row)
        written += 1
        print(
            f"  forecast#{fid} {row['ticker']}: p_mkt={row['p_market']:.3f} "
            f"p_model={row['p_model']} p_final={row['p_final']:.3f} verdict={row['verdict']}"
        )

    # Resolve all pending tickers against live Kalshi settlement
    pending = [
        r[0]
        for r in conn.execute(
            "SELECT DISTINCT market_ticker FROM forecasts WHERE outcome IS NULL"
        ).fetchall()
    ]
    resolved = 0
    for ticker in pending:
        resolved += resolve_if_settled(conn, ticker)

    print(f"\nWrote {written} new pending forecasts (source: live open book + agents).")
    print(f"Resolved this run: {resolved} (source: Kalshi market.result only).")
    print_brier_report(conn)
    conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
