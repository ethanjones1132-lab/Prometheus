#!/usr/bin/env python3
"""Edge / calibration autopsy on live predictions.db (read-only)."""
from __future__ import annotations

import math
import re
import sqlite3
from collections import Counter, defaultdict

DB = r"C:/Users/ethan/.openclaw/kalshi-monster/predictions.db"
FEE_MULT = 0.07
MIN_EDGE = 0.05
LAM = 0.25


def clamp(p: float) -> float:
    return max(0.01, min(0.99, p))


def logit(p: float) -> float:
    p = clamp(p)
    return math.log(p / (1.0 - p))


def shrink(pm: float, pk: float, lam: float = LAM) -> float:
    x = lam * logit(pm) + (1.0 - lam) * logit(pk)
    return 1.0 / (1.0 + math.exp(-x))


def entry(c: float) -> float:
    c = max(0.01, min(0.99, c))
    return c + FEE_MULT * c * (1.0 - c)


def series(t: str) -> str:
    m = re.match(r"^([A-Z0-9]+)", t or "")
    return m.group(1) if m else "UNK"


def main() -> None:
    con = sqlite3.connect(DB)
    con.row_factory = sqlite3.Row
    rows = con.execute(
        """
        SELECT market_ticker, p_market, p_model, p_final, outcome, verdict,
               event_key, brier_final, brier_market, brier_model, created_at
        FROM forecasts
        WHERE outcome IS NOT NULL AND p_model IS NOT NULL
        """
    ).fetchall()
    print(f"resolved with p_model: {len(rows)}")

    yes_trades = []
    no_trades = []
    for r in rows:
        pmkt = float(r["p_market"])
        pmod = float(r["p_model"])
        pf = float(r["p_final"])
        o = int(r["outcome"])
        ey = pf - entry(pmkt)
        en = (1.0 - pf) - entry(1.0 - pmkt)
        s = series(r["market_ticker"])
        if ey >= MIN_EDGE:
            pnl = (1.0 - entry(pmkt)) if o == 1 else -entry(pmkt)
            yes_trades.append((s, r["market_ticker"], ey, pf, pmkt, pmod, o, pnl, r["verdict"]))
        if en >= MIN_EDGE:
            en_cost = entry(1.0 - pmkt)
            pnl = (1.0 - en_cost) if o == 0 else -en_cost
            no_trades.append((s, r["market_ticker"], en, pf, pmkt, pmod, o, pnl, r["verdict"]))

    def summarize(name, trades):
        if not trades:
            print(f"{name}: n=0")
            return
        pnls = [t[7] for t in trades]
        wr = sum(1 for p in pnls if p > 0)
        print(
            f"{name}: n={len(trades)} total_pnl={sum(pnls):+.3f} "
            f"avg={sum(pnls)/len(pnls):+.3f} wr={wr}/{len(trades)}"
        )

    print("\n=== COUNTERFACTUAL PAPER (fee-aware mid entry, $1 payout) ===")
    summarize("trade_yes signals", yes_trades)
    summarize("trade_no signals", no_trades)

    print("\nYES by series:")
    by = defaultdict(list)
    for t in yes_trades:
        by[t[0]].append(t[7])
    for s, pnls in sorted(by.items(), key=lambda x: -len(x[1])):
        print(
            f"  {s}: n={len(pnls)} pnl={sum(pnls):+.3f} "
            f"avg={sum(pnls)/len(pnls):+.3f} wr={sum(1 for p in pnls if p>0)}/{len(pnls)}"
        )

    print("\nNO by series:")
    by = defaultdict(list)
    for t in no_trades:
        by[t[0]].append(t[7])
    for s, pnls in sorted(by.items(), key=lambda x: -len(x[1])):
        print(
            f"  {s}: n={len(pnls)} pnl={sum(pnls):+.3f} "
            f"avg={sum(pnls)/len(pnls):+.3f} wr={sum(1 for p in pnls if p>0)}/{len(pnls)}"
        )

    print("\n=== BEST-SIDE ONLY (max edge >= threshold) ===")
    for th in [0.05, 0.08, 0.10, 0.12, 0.15, 0.20]:
        sel = []
        for r in rows:
            pmkt = float(r["p_market"])
            pf = float(r["p_final"])
            o = int(r["outcome"])
            ey = pf - entry(pmkt)
            en = (1.0 - pf) - entry(1.0 - pmkt)
            if ey >= en and ey >= th:
                pnl = (1.0 - entry(pmkt)) if o == 1 else -entry(pmkt)
                sel.append((series(r["market_ticker"]), "YES", ey, pnl))
            elif en > ey and en >= th:
                pnl = (1.0 - entry(1.0 - pmkt)) if o == 0 else -entry(1.0 - pmkt)
                sel.append((series(r["market_ticker"]), "NO", en, pnl))
        if not sel:
            print(f"  th={th:.2f}: n=0")
            continue
        pnls = [t[3] for t in sel]
        print(
            f"  th={th:.2f}: n={len(sel)} pnl={sum(pnls):+.3f} "
            f"avg={sum(pnls)/len(pnls):+.3f} wr={sum(1 for p in pnls if p>0)}/{len(sel)}"
        )
        if th == 0.05:
            by = defaultdict(list)
            for t in sel:
                by[f"{t[0]}:{t[1]}"].append(t[3])
            for k, ps in sorted(by.items(), key=lambda x: sum(x[1]), reverse=True):
                print(f"    {k}: n={len(ps)} pnl={sum(ps):+.3f} avg={sum(ps)/len(ps):+.3f}")

    print("\n=== MODEL BIAS (mean p - outcome; + = overconfident YES) ===")
    errs_m = [float(r["p_model"]) - float(r["outcome"]) for r in rows]
    errs_f = [float(r["p_final"]) - float(r["outcome"]) for r in rows]
    errs_k = [float(r["p_market"]) - float(r["outcome"]) for r in rows]
    n = len(rows)
    print(f"mean(p_model - y)={sum(errs_m)/n:+.4f}")
    print(f"mean(p_final - y)={sum(errs_f)/n:+.4f}")
    print(f"mean(p_market - y)={sum(errs_k)/n:+.4f}")

    print("\nBy market bucket:")
    for lo, hi in [(0, 0.15), (0.15, 0.30), (0.30, 0.50), (0.50, 0.70), (0.70, 0.85), (0.85, 1.01)]:
        b = [r for r in rows if lo <= float(r["p_market"]) < hi]
        if not b:
            continue
        my = sum(float(r["outcome"]) for r in b) / len(b)
        mm = sum(float(r["p_model"]) for r in b) / len(b)
        mf = sum(float(r["p_final"]) for r in b) / len(b)
        mk = sum(float(r["p_market"]) for r in b) / len(b)
        bf = sum((float(r["p_final"]) - float(r["outcome"])) ** 2 for r in b) / len(b)
        bm = sum((float(r["p_market"]) - float(r["outcome"])) ** 2 for r in b) / len(b)
        bmo = sum((float(r["p_model"]) - float(r["outcome"])) ** 2 for r in b) / len(b)
        print(
            f"  mkt[{lo:.2f},{hi:.2f}): n={len(b)} y={my:.2f} mkt={mk:.2f} "
            f"model={mm:.2f} final={mf:.2f} | Bf={bf:.3f} Bm={bm:.3f} Bmo={bmo:.3f} "
            f"edge={bm-bf:+.4f}"
        )

    print("\n=== LAMBDA SWEEP (Brier on model-bearing resolved) ===")
    for lam in [0.05, 0.10, 0.15, 0.20, 0.25, 0.35, 0.50, 0.75, 1.0]:
        bf = bm = 0.0
        for r in rows:
            pf = shrink(float(r["p_model"]), float(r["p_market"]), lam)
            o = float(r["outcome"])
            bf += (pf - o) ** 2
            bm += (float(r["p_market"]) - o) ** 2
        print(f"  lam={lam:.2f}: Bf={bf/n:.4f} Bm={bm/n:.4f} delta={bm/n - bf/n:+.4f}")

    print("\n=== FADE MODEL when |pmod-pmkt| >= 0.25 ===")
    fade = []
    for r in rows:
        pmkt = float(r["p_market"])
        pmod = float(r["p_model"])
        o = int(r["outcome"])
        if abs(pmod - pmkt) < 0.25:
            continue
        if pmod > pmkt:
            pnl = (1.0 - entry(1.0 - pmkt)) if o == 0 else -entry(1.0 - pmkt)
            fade.append(("FADE_YES", series(r["market_ticker"]), pnl))
        else:
            pnl = (1.0 - entry(pmkt)) if o == 1 else -entry(pmkt)
            fade.append(("FADE_NO", series(r["market_ticker"]), pnl))
    if fade:
        pnls = [t[2] for t in fade]
        print(
            f"n={len(fade)} pnl={sum(pnls):+.3f} avg={sum(pnls)/len(pnls):+.3f} "
            f"wr={sum(1 for p in pnls if p>0)}/{len(fade)}"
        )
        by = defaultdict(list)
        for t in fade:
            by[f"{t[0]}:{t[1]}"].append(t[2])
        for k, ps in sorted(by.items(), key=lambda x: sum(x[1]), reverse=True):
            print(f"  {k}: n={len(ps)} pnl={sum(ps):+.3f}")

    print("\n=== LEDGER verdict=trade_yes only (as pipeline logged) ===")
    ty = [r for r in rows if r["verdict"] == "trade_yes"]
    pnls = []
    for r in ty:
        pmkt = float(r["p_market"])
        o = int(r["outcome"])
        pnl = (1.0 - entry(pmkt)) if o == 1 else -entry(pmkt)
        pnls.append(pnl)
        mark = "W" if pnl > 0 else "L"
        print(
            f"  {mark} pnl={pnl:+.3f} mkt={pmkt:.3f} final={float(r['p_final']):.3f} "
            f"model={float(r['p_model']):.3f} {r['market_ticker']}"
        )
    print(f"total PnL if papered all trade_yes: {sum(pnls):+.3f} (n={len(pnls)})")

    print("\n=== If pipeline matched Rust evaluate() on same rows ===")
    vc = Counter()
    for r in rows:
        pmkt = float(r["p_market"])
        pf = float(r["p_final"])
        ey = pf - entry(pmkt)
        en = (1.0 - pf) - entry(1.0 - pmkt)
        if ey >= MIN_EDGE and ey >= en:
            v = "trade_yes"
        elif en >= MIN_EDGE:
            v = "trade_no"
        else:
            v = "pass"
        vc[v] += 1
    print("rust-style:", dict(vc))
    print("actual ledger:", dict(Counter(r["verdict"] for r in rows)))

    # Reliability-style: when model is very bullish on cheap contracts
    print("\n=== CHEAP YES (<0.25 mkt) where model - mkt >= 0.30 ===")
    cheap = [
        r
        for r in rows
        if float(r["p_market"]) < 0.25 and float(r["p_model"]) - float(r["p_market"]) >= 0.30
    ]
    if cheap:
        y = sum(int(r["outcome"]) for r in cheap) / len(cheap)
        print(
            f"n={len(cheap)} realized_yes_rate={y:.2%} "
            f"avg_mkt={sum(float(r['p_market']) for r in cheap)/len(cheap):.3f} "
            f"avg_model={sum(float(r['p_model']) for r in cheap)/len(cheap):.3f}"
        )
        # Buying NO on these
        pnls = []
        for r in cheap:
            pmkt = float(r["p_market"])
            o = int(r["outcome"])
            pnl = (1.0 - entry(1.0 - pmkt)) if o == 0 else -entry(1.0 - pmkt)
            pnls.append(pnl)
        print(
            f"  if BUY NO on all: pnl={sum(pnls):+.3f} avg={sum(pnls)/len(pnls):+.3f} "
            f"wr={sum(1 for p in pnls if p>0)}/{len(pnls)}"
        )

    # Real tape length from snapshots?
    n_snap = con.execute("SELECT COUNT(*) FROM kalshi_price_snapshots").fetchone()[0]
    print(f"\nprice snapshots available: {n_snap}")
    # Sample tickers with most snapshots
    tops = con.execute(
        """
        SELECT ticker, COUNT(*) c,
               MIN(snapshot_at) mn, MAX(snapshot_at) mx
        FROM kalshi_price_snapshots
        GROUP BY ticker ORDER BY c DESC LIMIT 10
        """
    ).fetchall()
    print("top tape tickers:")
    for t in tops:
        print(f"  {t[0]}: {t[1]} snaps {t[2]} -> {t[3]}")


if __name__ == "__main__":
    main()
