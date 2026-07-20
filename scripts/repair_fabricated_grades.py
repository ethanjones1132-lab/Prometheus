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
  - json_extract(full_decision_json, '$.ticker'), trimmed, is non-empty —
    the repo's standard ticker gate (matches ml_predictor.rs's
    KALSHI_TICKER_PREDICATE constant), so this script only ever touches
    rows that genuinely carry a Kalshi decision, AND
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

# The repo's standard ticker gate (matches ml_predictor.rs's
# KALSHI_TICKER_PREDICATE constant: full_decision_json IS NOT NULL AND
# trim(json_extract(full_decision_json, '$.ticker')) != ''), plus the
# outcome/decision-JSON conditions this script applies in Python via
# is_no_position(). Printed verbatim before any write so a reviewer can see
# exactly what was matched without reading the source.
SELECTION_CRITERIA = (
    "SQL: outcome IN ('Win','Loss') "
    "AND full_decision_json IS NOT NULL "
    "AND trim(json_extract(full_decision_json, '$.ticker')) != '' "
    "-- then in Python: is_no_position(contract_side, decision, resolve_stake(recommended_stake_dollars)) is True"
)


def find_candidates(conn: sqlite3.Connection) -> list[dict]:
    """Read-only scan for rows that need repair."""
    rows = conn.execute(
        """
        SELECT id, outcome, actual_result, clv, notes, full_decision_json
        FROM predictions
        WHERE outcome IN ('Win', 'Loss')
          AND full_decision_json IS NOT NULL
          AND trim(json_extract(full_decision_json, '$.ticker')) != ''
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


def backup_db(db_path: Path) -> Path:
    """Back up the live DB to a timestamped sibling file before any write.

    Uses sqlite3.Connection.backup() (an online/hot backup — safe even
    while the Tauri app has the DB open) rather than a plain file copy.
    This mutates a live financial-record DB; reversibility is worth the
    three extra lines.
    """
    ts = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S")
    backup_path = db_path.with_name(f"{db_path.name}.bak-{ts}-repair")
    src = sqlite3.connect(str(db_path))
    try:
        dst = sqlite3.connect(str(backup_path))
        try:
            src.backup(dst)
        finally:
            dst.close()
    finally:
        src.close()
    return backup_path


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
    print(f"Selection criteria: {SELECTION_CRITERIA}")
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

    backup_path = backup_db(DB)
    print(f"\nBacked up DB to: {backup_path}")

    rw_conn = sqlite3.connect(str(DB))
    try:
        with rw_conn:
            for c in candidates:
                repair(rw_conn, c)
    finally:
        rw_conn.close()
    print(f"Repaired {len(candidates)} row(s). Criteria applied: {SELECTION_CRITERIA}")


if __name__ == "__main__":
    main()
