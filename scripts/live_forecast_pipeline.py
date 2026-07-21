#!/usr/bin/env python3
"""Live forecast pipeline smoke — REAL Kalshi + REAL agents only.

Does NOT fabricate outcomes. Writes pending forecast rows into the app's
SQLite ledger (~/.openclaw/kalshi-monster/predictions.db) using:
  - open market mids from https://api.elections.kalshi.com/trade-api/v2/markets
  - agent signals from local fincept-sidecar agents (yfinance / contract mids)
  - log-odds shrinkage λ=0.25 matching edge_engine::shrink

Resolves only when Kalshi already reports result Yes/No for that ticker.
"""

from __future__ import annotations

# Cron hosts (Hermes) inject a broken hermes-agent venv onto PYTHONPATH.
# That shadows fincept-sidecar's pydantic_core native ext. Drop hermes paths
# before any third-party import so the fincept/.venv (or system) wins.
import os as _os
import sys as _sys
_pp = _os.environ.get("PYTHONPATH", "")
if _pp:
    _parts = [p for p in _pp.split(_os.pathsep) if p and "hermes-agent" not in p.replace("\\", "/").lower()]
    if _parts:
        _os.environ["PYTHONPATH"] = _os.pathsep.join(_parts)
    else:
        _os.environ.pop("PYTHONPATH", None)
    # Also scrub already-populated sys.path entries from the parent process
    _sys.path[:] = [p for p in _sys.path if p and "hermes-agent" not in p.replace("\\", "/").lower()]
import asyncio
import json
import math
import os
import sqlite3
import sys
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

# Allow importing agents from fincept-sidecar
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "fincept-sidecar"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from kalshi_ticker import (  # noqa: E402
    ensure_forecasts_table,
    find_duplicate_forecast,
    provenance_for,
)

from agents.orchestrator import collect_market_opinion  # noqa: E402
from fincept_sidecar.schemas import MarketCategory, MarketOpinionRequest  # noqa: E402

KALSHI_MARKETS = "https://api.elections.kalshi.com/trade-api/v2/markets"
LAMBDA = 0.25
SOURCE = "live_forecast_pipeline.py"
DB = Path(os.environ.get("USERPROFILE", os.environ.get("HOME", "."))) / ".openclaw/kalshi-monster/predictions.db"


def clamp(p: float) -> float:
    return max(0.01, min(0.99, p))


def logit(p: float) -> float:
    p = clamp(p)
    return math.log(p / (1.0 - p))


def shrink(p_model: float, p_market: float, lam: float = LAMBDA) -> float:
    x = lam * logit(p_model) + (1.0 - lam) * logit(p_market)
    return 1.0 / (1.0 + math.exp(-x))


def http_json(url: str) -> dict:
    req = urllib.request.Request(url, headers={"User-Agent": "kalshi-monster-live-pipeline/0.1"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode())


# Series the technical agent can actually price (yfinance underlyings).
# Generic /markets?status=open is dominated by sports legs agents abstain on.
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


def fetch_open_markets(limit: int = 50) -> list[dict]:
    """Pull short-horizon markets from agent-analyzable series first.

    Falls back to the unfiltered open book only if every preferred series is empty.
    """
    from urllib.parse import urlencode

    collected: list[dict] = []
    seen: set[str] = set()
    for series in PREFERRED_SERIES:
        params = urlencode(
            {
                "limit": 100,
                "status": "open",
                "series_ticker": series,
                "mve_filter": "exclude",
            }
        )
        try:
            data = http_json(f"{KALSHI_MARKETS}?{params}")
        except Exception as e:
            print(f"  WARN series {series}: {e}")
            continue
        markets = data.get("markets") or []
        print(f"  series {series}: {len(markets)} open")
        for m in markets:
            t = m.get("ticker") or ""
            if not t or t in seen:
                continue
            seen.add(t)
            collected.append(m)
        if len(collected) >= limit * 3:
            break
    if not collected:
        print("  preferred series empty — falling back to unfiltered open book")
        data = http_json(f"{KALSHI_MARKETS}?limit={limit}&status=open&mve_filter=exclude")
        collected = data.get("markets") or []
    return collected


def fetch_market(ticker: str) -> dict:
    url = f"{KALSHI_MARKETS}/{urllib.request.quote(ticker)}"
    data = http_json(url)
    return data.get("market") or data


def mid_of(m: dict) -> float | None:
    try:
        bid = float(m.get("yes_bid_dollars") or m.get("yes_bid") or 0)
        ask = float(m.get("yes_ask_dollars") or m.get("yes_ask") or 0)
    except (TypeError, ValueError):
        return None
    if bid <= 0 and ask <= 0:
        return None
    if bid <= 0:
        return clamp(ask)
    if ask <= 0:
        return clamp(bid)
    return clamp(0.5 * (bid + ask))


def load_contract_mids(ticker: str, limit: int = 24) -> list[float] | None:
    """Pull real mid history from predictions.db price snapshots when available.

    Returns None when history is too thin — callers must NOT fabricate a flat
    series (that lied to contract_tape about momentum + confidence).
    """
    try:
        conn = sqlite3.connect(str(DB))
        rows = conn.execute(
            """
            SELECT yes_prob_pct FROM kalshi_price_snapshots
            WHERE ticker = ?
            ORDER BY snapshot_at ASC
            """,
            (ticker,),
        ).fetchall()
        conn.close()
    except Exception:
        return None
    mids = []
    for (pct,) in rows:
        try:
            p = float(pct) / 100.0 if float(pct) > 1.0 else float(pct)
        except (TypeError, ValueError):
            continue
        if 0.0 < p < 1.0:
            mids.append(p)
    if len(mids) < 3:
        return None
    return mids[-limit:]


def ensure_forecast_table(conn: sqlite3.Connection) -> None:
    """Schema lives in `kalshi_ticker.FORECASTS_DDL`, not inlined here — three
    scripts write to this table and hand-copied DDL is how they drift."""
    ensure_forecasts_table(conn)


def insert_forecast(
    conn: sqlite3.Connection,
    *,
    ticker: str,
    close_time: str,
    p_market: float,
    p_model: float | None,
    p_final: float,
    verdict: str,
    reasons: list[str],
    breakdown: dict | None,
    agents_opining: int | None = None,
) -> tuple[int, bool]:
    """Insert one forecast row.

    Returns ``(forecast_id, inserted)``. ``inserted`` is False when the row was
    suppressed as a duplicate — callers must not count those as writes, or the
    run summary over-reports how much evidence it added.

    Provenance is recorded on the way in: which event the ticker belongs to,
    when that event started, and whether this row was written after the tape
    went live. Without it a row written mid-game is indistinguishable from a
    genuine pre-event forecast, and the calibration gate counts both.
    """
    created_at = datetime.now(timezone.utc).isoformat()
    dup = find_duplicate_forecast(conn, ticker, p_market, created_at)
    if dup is not None:
        return dup, False

    event_key, event_start_at, in_play = provenance_for(ticker, created_at)
    cur = conn.execute(
        """
        INSERT INTO forecasts (
            market_ticker, created_at, close_time,
            p_market, p_model, p_final, verdict, verdict_reasons,
            stake_suggested, agent_breakdown,
            event_start_at, is_in_play, source, event_key, agents_opining
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?)
        """,
        (
            ticker,
            created_at,
            close_time,
            p_market,
            p_model,
            p_final,
            verdict,
            json.dumps(reasons),
            json.dumps(breakdown) if breakdown else None,
            event_start_at,
            in_play,
            SOURCE,
            event_key,
            agents_opining,
        ),
    )
    conn.commit()
    return int(cur.lastrowid), True


def resolve_if_settled(conn: sqlite3.Connection, ticker: str) -> int:
    m = fetch_market(ticker)
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
    conn.commit()
    return n


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

    # Kalshi API contract geometry — critical for B-leg brackets vs T-leg thresholds.
    floor_strike = m.get("floor_strike")
    cap_strike = m.get("cap_strike")
    try:
        floor_strike = float(floor_strike) if floor_strike is not None else None
    except (TypeError, ValueError):
        floor_strike = None
    try:
        cap_strike = float(cap_strike) if cap_strike is not None else None
    except (TypeError, ValueError):
        cap_strike = None

    # Honest tape: only pass real mid history. Fabricating [mid]*5 made
    # contract_tape claim momentum=0 with fake n=5 confidence.
    contract_mids = load_contract_mids(ticker)

    ctx: dict = {
        "depth": "standard",
    }
    if floor_strike is not None:
        ctx["floor_strike"] = floor_strike
    if cap_strike is not None:
        ctx["cap_strike"] = cap_strike
    if contract_mids is not None:
        ctx["contract_mids"] = contract_mids

    req = MarketOpinionRequest(
        market_ticker=ticker,
        title=title,
        resolution_rules=rules,
        close_time=close if "T" in str(close) else datetime.now(timezone.utc).isoformat(),
        category=category_for(title, ticker),
        yes_bid=max(0.0, min(1.0, yes_bid or p_market)),
        yes_ask=max(0.0, min(1.0, yes_ask or p_market)),
        context=ctx,
    )
    # parse close_time properly
    try:
        if isinstance(close, str):
            req.close_time = datetime.fromisoformat(close.replace("Z", "+00:00"))
    except ValueError:
        pass

    resp = await collect_market_opinion(req)
    opining = [s for s in resp.signals if s.probability is not None]
    if not opining:
        p_model = None
        p_final = p_market
        reasons = ["no agent opinion; p_final=p_market (honest market-only row)"]
        verdict = "pass"
        n_opining = 0
        edge_yes = edge_no = None
    else:
        # Confidence-weighted log-odds pool (matches prior pipeline; Rust uses routing weights).
        wsum = 0.0
        pooled = 0.0
        conf_max = 0.0
        for s in opining:
            w = max(0.01, s.confidence)
            pooled += w * logit(s.probability)
            wsum += w
            conf_max = max(conf_max, float(s.confidence or 0.0))
        p_model = 1.0 / (1.0 + math.exp(-(pooled / wsum)))
        p_final = shrink(p_model, p_market)
        reasons = [f"agents_opining={len(opining)}", f"lambda={LAMBDA}", f"conf_max={conf_max:.2f}"]
        n_opining = len(opining)

        # Rust edge_engine::evaluate parity: fee-aware both sides + min_confidence.
        min_conf = 0.30
        theta = 0.05
        fee_mult = 0.07
        ask_yes = yes_ask if yes_ask > 0 else p_market
        ask_no = (1.0 - yes_bid) if yes_bid > 0 else (1.0 - p_market)
        entry_yes = ask_yes + fee_mult * ask_yes * (1.0 - ask_yes)
        entry_no = ask_no + fee_mult * ask_no * (1.0 - ask_no)
        edge_yes = p_final - entry_yes
        edge_no = (1.0 - p_final) - entry_no
        if conf_max < min_conf:
            verdict = "pass"
            reasons.append(f"ensemble conf_max {conf_max:.2f} < min_confidence {min_conf:.2f}")
        elif edge_yes >= theta and edge_yes >= edge_no:
            verdict = "trade_yes"
            reasons.append(f"net YES edge {edge_yes*100:.1f}c >= {theta*100:.1f}c")
        elif edge_no >= theta:
            verdict = "trade_no"
            reasons.append(f"net NO edge {edge_no*100:.1f}c >= {theta*100:.1f}c")
        else:
            verdict = "pass"
            reasons.append(
                f"best net edge {max(edge_yes, edge_no)*100:.1f}c below {theta*100:.1f}c"
            )

    return {
        "ticker": ticker,
        "title": title,
        "close_time": close if isinstance(close, str) else str(close),
        "p_market": p_market,
        "p_model": p_model,
        "p_final": p_final,
        "verdict": verdict,
        "reasons": reasons,
        "agents_opining": n_opining,
        "edge_yes": edge_yes,
        "edge_no": edge_no,
        "floor_strike": floor_strike,
        "cap_strike": cap_strike,
        "breakdown": {
            "signals": [
                {
                    "agent": s.agent,
                    "probability": s.probability,
                    "confidence": s.confidence,
                    "inputs": [i.source for i in s.inputs_used],
                    "caveats": list(s.caveats or []),
                }
                for s in resp.signals
            ],
            "source": "live_forecast_pipeline.py + Kalshi public API + yfinance agents",
            "contract": {
                "floor_strike": floor_strike,
                "cap_strike": cap_strike,
            },
        },
    }


async def main() -> int:
    print(f"DB: {DB}")
    print(f"Kalshi open markets source: {KALSHI_MARKETS}?status=open")
    markets = fetch_open_markets(80)
    print(f"Fetched {len(markets)} open markets from Kalshi API")
    if not markets:
        print("No markets — abort")
        return 1

    # Prefer liquid short-horizon legs agents can price (not empty-book extremes).
    # Near-spot brackets (market mid not extreme) are where density models can
    # actually beat the tape; deep OTM bins are fee-dominated lottery tickets.
    def _rank_key(m: dict):
        mid = mid_of(m)
        close = m.get("close_time") or ""
        try:
            ct = datetime.fromisoformat(str(close).replace("Z", "+00:00"))
            days = (ct - datetime.now(timezone.utc)).total_seconds() / 86400.0
        except Exception:
            days = 99.0
        # Prefer mid in (0.08, 0.40) for brackets (actionable density), then
        # broader (0.08, 0.92), then sooner close, then volume.
        if mid is not None and 0.08 < mid < 0.40:
            mid_penalty = 0
        elif mid is not None and 0.08 < mid < 0.92:
            mid_penalty = 1
        else:
            mid_penalty = 2
        # Prefer contracts with API geometry (floor/cap) so technical can opine correctly.
        has_geom = 0 if (m.get("floor_strike") is not None or m.get("cap_strike") is not None) else 1
        vol = -float(m.get("volume_24h_fp") or m.get("volume_24h") or 0)
        return (has_geom, mid_penalty, days if days >= 0 else 99.0, vol)

    ranked = sorted(markets, key=_rank_key)

    conn = sqlite3.connect(str(DB))
    ensure_forecast_table(conn)

    written = 0
    model_written = 0
    # Scan more candidates; stop once we have enough model-bearing rows.
    target_model = 12
    for m in ranked[:40]:
        try:
            row = await analyze_one(m)
        except Exception as e:
            print(f"  skip {m.get('ticker')}: {e}")
            continue
        if not row:
            continue
        fid, inserted = insert_forecast(
            conn,
            ticker=row["ticker"],
            close_time=row["close_time"],
            p_market=row["p_market"],
            p_model=row["p_model"],
            p_final=row["p_final"],
            verdict=row["verdict"],
            reasons=row["reasons"],
            breakdown=row["breakdown"],
            agents_opining=row.get("agents_opining"),
        )
        if not inserted:
            print(f"  duplicate suppressed {row['ticker']} (matches forecast#{fid})")
            continue
        # Prefer model-bearing rows for the eligible gate; still log market-only
        # so the run is auditable, but they never count toward n_eligible.
        written += 1
        if row["p_model"] is not None:
            model_written += 1
        print(
            f"  forecast#{fid} {row['ticker']}: p_mkt={row['p_market']:.3f} "
            f"p_model={row['p_model']} p_final={row['p_final']:.3f} "
            f"verdict={row['verdict']} agents={row.get('agents_opining')}"
        )
        if model_written >= target_model:
            print(f"  hit target_model={target_model}; stopping scan")
            break

    # Resolve any pending tickers that Kalshi has already settled
    pending = conn.execute(
        "SELECT DISTINCT market_ticker FROM forecasts WHERE outcome IS NULL"
    ).fetchall()
    resolved = 0
    for (ticker,) in pending:
        try:
            resolved += resolve_if_settled(conn, ticker)
        except Exception as e:
            print(f"  resolve skip {ticker}: {e}")

    # Report
    n_res = conn.execute("SELECT COUNT(*) FROM forecasts WHERE outcome IS NOT NULL").fetchone()[0]
    n_un = conn.execute("SELECT COUNT(*) FROM forecasts WHERE outcome IS NULL").fetchone()[0]
    from kalshi_ticker import eligible_resolved_rows

    print(f"\nWrote {written} new forecast rows ({model_written} with p_model).")
    print(f"Resolved this run: {resolved}")
    print(f"Ledger totals: resolved={n_res} unresolved={n_un}")

    if n_res > 0:
        rows = conn.execute(
            "SELECT p_market, p_model, p_final, outcome FROM forecasts WHERE outcome IS NOT NULL"
        ).fetchall()
        bf = sum((p_f - o) ** 2 for _, _, p_f, o in rows) / len(rows)
        bm = sum((p_m - o) ** 2 for p_m, _, _, o in rows) / len(rows)
        model_rows = [(p_mod, p_m, o) for p_m, p_mod, _, o in rows if p_mod is not None]
        print(f"[raw] Brier(p_final)={bf:.4f}  Brier(p_market)={bm:.4f}  n={len(rows)}")
        if model_rows:
            bmod = sum((pm - o) ** 2 for pm, _, o in model_rows) / len(model_rows)
            bm_m = sum((p_m - o) ** 2 for _, p_m, o in model_rows) / len(model_rows)
            print(
                f"[raw] Brier(p_model)={bmod:.4f} on n_model={len(model_rows)}  "
                f"Brier(market|model)={bm_m:.4f}"
            )
        eligible = eligible_resolved_rows(conn)
        n_elig = len(eligible)
        if n_elig:
            ebf = sum((r[2] - r[3]) ** 2 for r in eligible) / n_elig
            ebm = sum((r[0] - r[3]) ** 2 for r in eligible) / n_elig
            print(
                f"[eligible] n={n_elig}/200  Brier(p_final)={ebf:.4f}  "
                f"Brier(p_market)={ebm:.4f}"
            )
            status = "OPEN candidate" if n_elig >= 200 and ebf <= ebm else "LOCKED"
        else:
            status = "LOCKED"
            print("[eligible] n=0/200 — no model-bearing pre-event rows yet")
        print(f"Gate (eligible ≥200 + Brier_final≤market + paper P&L>0): {status}")
    else:
        print("No resolved forecasts yet — calibration bar not started counting outcomes (honest).")

    conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
