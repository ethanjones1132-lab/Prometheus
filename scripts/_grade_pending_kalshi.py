#!/usr/bin/env python3
"""Grade all pending Kalshi predictions whose markets have settled."""
from __future__ import annotations

import json
import os
import sqlite3
import sys
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from grading_common import compute_clv, contract_pnl, is_no_position, resolve_stake

DB = Path(os.environ.get("USERPROFILE", os.environ.get("HOME", "."))) / ".openclaw/kalshi-monster/predictions.db"
KALSHI = "https://api.elections.kalshi.com/trade-api/v2/markets"


def fetch_market(ticker: str) -> dict:
    url = f"{KALSHI}/{urllib.parse.quote(ticker, safe='')}"
    req = urllib.request.Request(url, headers={"User-Agent": "kalshi-monster-grade/1.0"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode())
    return data.get("market") or data


def normalize_result(raw: str) -> str | None:
    t = (raw or "").strip().lower()
    if t in ("yes", "y", "true", "1"):
        return "Yes"
    if t in ("no", "n", "false", "0"):
        return "No"
    return None


def entry_for_side(dec: dict, side: str) -> float:
    pte = dec.get("price_to_enter")
    if isinstance(pte, (int, float)) and 0 < float(pte) <= 1:
        return float(pte)
    if isinstance(pte, (int, float)) and 1 < float(pte) <= 100:
        return float(pte) / 100.0
    mkt = dec.get("market_price_pct")
    if isinstance(mkt, (int, float)):
        yes = float(mkt) / 100.0 if float(mkt) > 1 else float(mkt)
        yes = max(0.01, min(0.99, yes))
        return max(0.01, min(0.99, 1.0 - yes)) if side == "NO" else yes
    return 0.5


def main() -> None:
    conn = sqlite3.connect(str(DB))
    conn.row_factory = sqlite3.Row
    rows = list(
        conn.execute(
            """
            SELECT id, player_name, pick_type, outcome, created_at, full_decision_json, entry_price, line
            FROM predictions
            WHERE (outcome IS NULL OR outcome = '' OR outcome = 'Pending')
              AND full_decision_json IS NOT NULL
            ORDER BY created_at DESC
            """
        )
    )
    print(f"Pending Kalshi decision rows: {len(rows)}")
    graded = []

    for r in rows:
        try:
            dec = json.loads(r["full_decision_json"])
        except json.JSONDecodeError:
            continue
        ticker = str(dec.get("ticker") or r["player_name"] or "")
        if not ticker.upper().startswith("KX"):
            continue
        if "TICKER" in ticker.upper() and len(ticker) < 20:
            print(f"SKIP placeholder {ticker}")
            continue

        side = str(dec.get("contract_side") or "").upper()
        decision = str(dec.get("decision") or "").upper()

        try:
            market = fetch_market(ticker)
        except urllib.error.HTTPError as e:
            print(f"SKIP {ticker}: HTTP {e.code}")
            continue
        except Exception as e:
            print(f"SKIP {ticker}: {e}")
            continue

        actual = normalize_result(market.get("result") or "")
        print(
            f"{ticker} status={market.get('status')} result={market.get('result')!r} "
            f"side={side} decision={decision}"
        )
        if not actual:
            print("  not settled — leave Pending")
            continue

        now = datetime.now(timezone.utc).isoformat()
        close = 1.0 if actual == "Yes" else 0.0
        stake = resolve_stake(dec.get("recommended_stake_dollars"))
        no_position = is_no_position(side, decision, stake=stake)
        if no_position:
            outcome, pnl, entry, stake, won, clv = "Push", 0.0, 0.0, 0.0, None, 0.0
        else:
            won = (side == "YES" and actual == "Yes") or (side == "NO" and actual == "No")
            entry = entry_for_side(dec, side)
            pnl = contract_pnl(stake, entry, bool(won))
            outcome = "Win" if won else "Loss"
            clv = compute_clv(side, entry, close)

        notes = f"Outcome: {actual}, PnL: {pnl}"

        conn.execute(
            """
            UPDATE predictions
            SET outcome = ?, actual_result = ?, notes = ?, resolved_at = ?,
                close_price = ?, clv = ?
            WHERE id = ?
            """,
            (outcome, pnl, notes, now, close, clv, r["id"]),
        )

        oi = 1 if actual == "Yes" else 0
        for fr in conn.execute(
            "SELECT id, p_market, p_model, p_final FROM forecasts WHERE market_ticker = ? AND outcome IS NULL",
            (ticker,),
        ):
            fid, pm, pmod, pf = fr

            def brier(p):
                return (float(p) - oi) ** 2 if p is not None else None

            conn.execute(
                """
                UPDATE forecasts
                SET resolved_at = ?, outcome = ?,
                    brier_market = ?, brier_model = ?, brier_final = ?
                WHERE id = ?
                """,
                (now, oi, brier(pm), brier(pmod), brier(pf), fid),
            )
            print(f"  forecast id={fid} brier_final={brier(pf)}")

        graded.append(
            {
                "id": r["id"],
                "ticker": ticker,
                "side": side,
                "decision": decision,
                "actual": actual,
                "outcome": outcome,
                "pnl": pnl,
                "stake": stake if side != "PASS" else 0,
                "entry": entry if side != "PASS" else None,
                "fair": dec.get("fair_probability_pct"),
                "mkt": dec.get("market_price_pct"),
                "thesis": (dec.get("thesis") or "")[:240],
                "created": r["created_at"],
            }
        )
        print(f"  -> {outcome} pnl={pnl:.4f}")

    conn.commit()

    # Also void/skip placeholder pending
    conn.execute(
        """
        UPDATE predictions
        SET outcome = 'Push', notes = 'Voided placeholder ticker', resolved_at = ?
        WHERE outcome = 'Pending' AND (player_name = 'KXEVENT-TICKER' OR player_name LIKE '%EVENT-TICKER%')
        """,
        (datetime.now(timezone.utc).isoformat(),),
    )
    conn.commit()

    print("\n=== GRADED SUMMARY ===")
    if not graded:
        print("No newly settled markets among pending Kalshi decisions.")
    for g in graded:
        print(
            f"{g['outcome']:5} {g['ticker']}  side={g['side']}  "
            f"actual={g['actual']}  fair={g['fair']} mkt={g['mkt']}  PnL={g['pnl']:.2f}"
        )
        print(f"      thesis: {g['thesis']}")

    print("\n=== Remaining Pending with decision JSON ===")
    for r in conn.execute(
        """
        SELECT player_name, created_at, outcome,
               json_extract(full_decision_json, '$.ticker') as t,
               json_extract(full_decision_json, '$.decision') as d
        FROM predictions
        WHERE outcome = 'Pending' AND full_decision_json IS NOT NULL
        """
    ):
        print(dict(r))

    conn.close()


if __name__ == "__main__":
    main()
