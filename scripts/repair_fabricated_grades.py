#!/usr/bin/env python3
"""Repair predictions rows that were graded as a real Win/Loss even though
the decision JSON says the engine declined to trade (PASS) or had no real
stake.

Background: _grade_latest.py's PASS guard only checked `contract_side`, not
`decision`. A decision can have contract_side="NO" while decision="PASS" —
meaning the engine identified a side but declined to trade it. That row fell
through to the betting branch and was booked as a fabricated Win with a
nonzero PnL and a nonzero CLV. scripts/grading_common.py now provides a
single is_no_position() predicate (used by both grading scripts) that
catches this; this script finds and repairs any row that was already
written to the DB before that fix existed.

A row is a repair candidate when:
  - full_decision_json is present, AND
  - it parses, AND
  - is_no_position(contract_side, decision, stake) is True per
    scripts/grading_common.py, AND
  - the stored outcome is 'Win' or 'Loss' (i.e. it was graded as a real bet)

Repair rewrites such a row to:
  outcome = 'Push', actual_result (pnl) = 0.0, clv = 0.0
  notes   = original notes + a recorded repair note (idempotent: repairing
            an already-Push row is a no-op because the WHERE clause only
            matches outcome IN ('Win','Loss'), so re-running never touches
            an already-repaired row twice).

This script intentionally does NOT depend on market_price_pct or the
forecasts.p_market value for either detection or repair — those two fields
are known to disagree for at least one ticker and that discrepancy is a
separate, later fix.

Usage:
    python repair_fabricated_grades.py            # dry run (default)
    python repair_fabricated_grades.py --dry-run   # explicit dry run
    python repair_fabricated_grades.py --write     # apply the repair
"""
from __future__ import annotations

import argparse
import json
import os
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from grading_common import is_no_position, resolve_stake

DB = Path(os.environ.get("USERPROFILE", os.environ.get("HOME", "."))) / ".openclaw/kalshi-monster/predictions.db"


def find_candidates(conn: sqlite3.Connection) -> list[dict]:
    """Read-only scan for rows that need repair."""
    rows = conn.execute(
        """
        SELECT id, outcome, actual_result, clv, notes, full_decision_json
        FROM predictions
        WHERE outcome IN ('Win', 'Loss')
          AND full_decision_json IS NOT NULL
        """
    ).fetchall()

    candidates = []
    for row in rows:
        try:
            dec = json.loads(row["full_decision_json"])
        except (json.JSONDecodeError, TypeError):
            continue
        side = dec.get("contract_side")
        decision_field = dec.get("decision")
        stake = resolve_stake(dec.get("recommended_stake_dollars"))
        if is_no_position(side, decision_field, stake=stake):
            candidates.append(
                {
                    "id": row["id"],
                    "ticker": dec.get("ticker"),
                    "old_outcome": row["outcome"],
                    "old_pnl": row["actual_result"],
                    "old_clv": row["clv"],
                    "old_notes": row["notes"],
                    "side": side,
                    "decision": decision_field,
                    "stake": stake,
                }
            )
    return candidates


def repair(conn: sqlite3.Connection, candidate: dict) -> None:
    now = datetime.now(timezone.utc).isoformat()
    repair_note = (
        f"[repaired {now}] was graded {candidate['old_outcome']} "
        f"(pnl={candidate['old_pnl']}, clv={candidate['old_clv']}) but decision JSON "
        f"shows contract_side={candidate['side']!r} decision={candidate['decision']!r} "
        f"recommended_stake_dollars resolved to {candidate['stake']} — engine declined "
        f"the trade; rewritten to Push / pnl 0.0 / clv 0.0."
    )
    old_notes = candidate["old_notes"] or ""
    new_notes = f"{old_notes}\n{repair_note}" if old_notes else repair_note
    conn.execute(
        """
        UPDATE predictions
        SET outcome = 'Push', actual_result = 0.0, clv = 0.0, notes = ?
        WHERE id = ?
        """,
        (new_notes, candidate["id"]),
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write",
        action="store_true",
        help="Apply the repair. Without this flag, the script always runs as a dry run.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Explicit dry run (this is also the default with no flags).",
    )
    args = parser.parse_args()

    write_mode = args.write and not args.dry_run

    ro_uri = f"file:{DB.as_posix()}?mode=ro"
    conn = sqlite3.connect(ro_uri, uri=True)
    conn.row_factory = sqlite3.Row

    candidates = find_candidates(conn)
    conn.close()

    print(f"DB: {DB}")
    print(f"Mode: {'WRITE' if write_mode else 'DRY RUN'}")
    print(f"Fabricated-grade candidates found: {len(candidates)}")
    for c in candidates:
        print(
            f"  id={c['id']}\n"
            f"    ticker={c['ticker']}\n"
            f"    stored: outcome={c['old_outcome']!r} pnl={c['old_pnl']} clv={c['old_clv']}\n"
            f"    decision json: contract_side={c['side']!r} decision={c['decision']!r} "
            f"stake_resolved={c['stake']}\n"
            f"    -> will rewrite to: outcome='Push' pnl=0.0 clv=0.0"
        )

    if not candidates:
        print("\nNothing to repair.")
        return

    if not write_mode:
        print(f"\nDry run only — no changes written. Re-run with --write to apply {len(candidates)} repair(s).")
        return

    rw_conn = sqlite3.connect(str(DB))
    try:
        for c in candidates:
            repair(rw_conn, c)
        rw_conn.commit()
    finally:
        rw_conn.close()
    print(f"\nRepaired {len(candidates)} row(s).")


if __name__ == "__main__":
    main()
