#!/usr/bin/env python3
"""Inspect latest predictions and grade settled ones against Kalshi public API."""
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


def http_json(url: str) -> dict:
    req = urllib.request.Request(url, headers={"User-Agent": "kalshi-monster-grade/1.0"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode())


def fetch_market(ticker: str) -> dict:
    url = f"{KALSHI}/{urllib.parse.quote(ticker, safe='')}"
    data = http_json(url)
    return data.get("market") or data


def normalize_result(raw: str) -> str | None:
    t = (raw or "").strip().lower()
    if t in ("yes", "y", "true", "1"):
        return "Yes"
    if t in ("no", "n", "false", "0"):
        return "No"
    return None


def parse_side(decision: dict | None, pick_type: str | None) -> str | None:
    if decision:
        side = str(decision.get("contract_side") or "").upper()
        if side in ("YES", "NO"):
            return side
        if side == "PASS":
            return "PASS"
    if pick_type:
        pt = pick_type.lower()
        if pt in ("over", "yes"):
            return "YES"
        if pt in ("under", "no"):
            return "NO"
        if pt == "pass":
            return "PASS"
    return None


def bet_won(side: str, actual: str) -> bool | None:
    if side == "YES":
        return actual == "Yes"
    if side == "NO":
        return actual == "No"
    return None


def entry_price(decision: dict | None, row: sqlite3.Row, side: str) -> float:
    if decision:
        pte = decision.get("price_to_enter")
        if isinstance(pte, (int, float)) and 0 < pte <= 1:
            return float(pte)
        if isinstance(pte, (int, float)) and 1 < pte <= 100:
            return float(pte) / 100.0
        mkt = decision.get("market_price_pct")
        if isinstance(mkt, (int, float)):
            yes = float(mkt) / 100.0 if mkt > 1 else float(mkt)
            if side == "NO":
                return max(0.01, min(0.99, 1.0 - yes))
            return max(0.01, min(0.99, yes))
    ep = row["entry_price"]
    if ep is not None and 0 < float(ep) <= 1:
        return float(ep)
    return 0.5


def main() -> None:
    conn = sqlite3.connect(str(DB))
    conn.row_factory = sqlite3.Row
    cur = conn.cursor()

    rows = list(
        cur.execute(
            """
            SELECT id, player_name, pick_type, probability, outcome, created_at,
                   full_decision_json, entry_price, line, confidence, reasoning
            FROM predictions
            ORDER BY created_at DESC
            LIMIT 15
            """
        )
    )
    print("=== Recent predictions ===")
    for r in rows:
        dec = None
        if r["full_decision_json"]:
            try:
                dec = json.loads(r["full_decision_json"])
            except json.JSONDecodeError:
                pass
        ticker = (dec or {}).get("ticker") or r["player_name"]
        print(
            f"  {r['created_at'][:19]}  {r['outcome']:8}  {ticker}  "
            f"pick={r['pick_type']} side={(dec or {}).get('contract_side')} "
            f"dec={(dec or {}).get('decision')}"
        )

    # Prefer latest pending with a real KX ticker / decision
    target = None
    for r in rows:
        if r["outcome"] not in (None, "", "Pending"):
            continue
        dec = None
        if r["full_decision_json"]:
            try:
                dec = json.loads(r["full_decision_json"])
            except json.JSONDecodeError:
                pass
        ticker = (dec or {}).get("ticker") or r["player_name"] or ""
        if not str(ticker).upper().startswith("KX"):
            continue
        if "TICKER" in str(ticker).upper() and "EVENT" in str(ticker).upper():
            continue
        target = (r, dec, str(ticker))
        break

    if not target:
        # Fall back to latest overall Kalshi-ish row even if already graded
        for r in rows:
            dec = None
            if r["full_decision_json"]:
                try:
                    dec = json.loads(r["full_decision_json"])
                except json.JSONDecodeError:
                    pass
            ticker = (dec or {}).get("ticker") or r["player_name"] or ""
            if str(ticker).upper().startswith("KX") and "TICKER" not in str(ticker).upper():
                target = (r, dec, str(ticker))
                break

    if not target:
        print("\nNo Kalshi-style prediction found to grade.")
        return

    row, dec, ticker = target
    print(f"\n=== Grading latest candidate ===")
    print(f"id={row['id']}")
    print(f"ticker={ticker}")
    print(f"created={row['created_at']}")
    print(f"current_outcome={row['outcome']}")
    if dec:
        print(
            f"decision={dec.get('decision')} side={dec.get('contract_side')} "
            f"fair={dec.get('fair_probability_pct')} mkt_pct={dec.get('market_price_pct')} "
            f"stake={dec.get('recommended_stake_dollars')}"
        )
        print(f"thesis={str(dec.get('thesis', ''))[:300]}")

    try:
        market = fetch_market(ticker)
    except urllib.error.HTTPError as e:
        print(f"\nKalshi fetch failed HTTP {e.code}: {e.reason}")
        print("Market may not exist or ticker is wrong — cannot grade.")
        return
    except Exception as e:
        print(f"\nKalshi fetch failed: {e}")
        return

    status = market.get("status")
    result_raw = market.get("result") or ""
    title = market.get("title") or market.get("yes_sub_title") or ""
    print(f"\nKalshi market: {title}")
    print(f"status={status} result={result_raw!r}")

    actual = normalize_result(result_raw)
    if not actual:
        print("\nMarket not settled yet (empty/non-binary result). Leaving Pending.")
        # Still resolve any forecasts if somehow result present
        return

    side = parse_side(dec, row["pick_type"])
    decision_field = (dec or {}).get("decision")
    stake = resolve_stake((dec or {}).get("recommended_stake_dollars"))
    if stake <= 0 and row["line"] is not None:
        stake = resolve_stake(row["line"])
    print(f"parsed side={side} actual={actual}")
    no_position = is_no_position(side, decision_field, stake=stake)
    if no_position:
        print("PASS / unknown side / zero stake — mark notes only, no Win/Loss PnL.")
        won = None
        pnl = 0.0
        clv = 0.0
        outcome_label = "Push"
    else:
        won = bet_won(side, actual)
        entry = entry_price(dec, row, side)
        pnl = contract_pnl(stake, entry, bool(won))
        close_price = 1.0 if actual == "Yes" else 0.0
        clv = compute_clv(side, entry, close_price)
        outcome_label = "Win" if won else "Loss"
        print(f"entry={entry:.4f} stake={stake:.2f} won={won} pnl={pnl:.4f}")

    now = datetime.now(timezone.utc).isoformat()
    notes = f"Outcome: {actual}, PnL: {pnl}"
    close_price = 1.0 if actual == "Yes" else 0.0

    # Write grade
    conn_w = sqlite3.connect(str(DB))
    conn_w.execute(
        """
        UPDATE predictions
        SET outcome = ?, actual_result = ?, notes = ?, resolved_at = ?,
            close_price = ?, clv = ?
        WHERE id = ?
        """,
        (outcome_label, pnl, notes, now, close_price, clv, row["id"]),
    )

    # Resolve matching forecast rows
    outcome_i = 1 if actual == "Yes" else 0
    frows = list(
        conn_w.execute(
            "SELECT id, p_market, p_model, p_final FROM forecasts WHERE market_ticker = ? AND outcome IS NULL",
            (ticker,),
        )
    )
    for fr in frows:
        fid, p_mkt, p_mod, p_fin = fr
        brier = lambda p: (float(p) - outcome_i) ** 2 if p is not None else None
        conn_w.execute(
            """
            UPDATE forecasts
            SET resolved_at = ?, outcome = ?,
                brier_market = ?, brier_model = ?, brier_final = ?
            WHERE id = ?
            """,
            (
                now,
                outcome_i,
                brier(p_mkt),
                brier(p_mod) if p_mod is not None else None,
                brier(p_fin),
                fid,
            ),
        )
        print(f"  resolved forecast id={fid} brier_final={brier(p_fin)}")

    conn_w.commit()
    conn_w.close()

    print(f"\n=== GRADE: {outcome_label} ===")
    print(f"ticker={ticker} actual={actual} side={side} pnl={pnl:.4f}")
    print(f"prediction id={row['id']} updated at {now}")


if __name__ == "__main__":
    main()
