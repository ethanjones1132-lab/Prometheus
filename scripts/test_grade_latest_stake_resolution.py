"""Regression test for _grade_latest.py's own stake-resolution glue logic
(resolve_row_stake), not just grading_common.py.

Bug found in spec review: resolve_row_stake() fell back to row["line"] when
recommended_stake_dollars was absent/zero. `line` is an overloaded DB
column — tracker.rs's Kalshi decision path stores stake there, but the
sports-prop-parsing path stores the prop line itself (e.g. 285.5, 235.5,
70.5). A row with a real prop `line` but no recommended stake would be
graded as a bet sized off an unrelated number, and — because this ran
*before* is_no_position() — could flip a genuine no-position row into a
booked bet.

This test exercises _grade_latest.resolve_row_stake() directly (the real
grader code, not a reimplementation of it) so it fails if the line fallback
is ever reintroduced.
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from _grade_latest import resolve_row_stake
from grading_common import is_no_position


def test_stake_resolution_ignores_overloaded_line_column_when_stake_absent():
    # decision is a real BUY (not PASS); recommended_stake_dollars is
    # absent; row["line"] holds an unrelated sports-prop line (285.5).
    # Stake must resolve to 0.0 — never 285.5.
    dec = {"contract_side": "YES", "decision": "BUY"}
    row = {"line": 285.5}
    stake = resolve_row_stake(dec, row)
    assert stake == 0.0


def test_stake_resolution_ignores_overloaded_line_column_when_stake_is_zero():
    dec = {"contract_side": "NO", "decision": "BUY", "recommended_stake_dollars": 0.0}
    row = {"line": 70.5}
    stake = resolve_row_stake(dec, row)
    assert stake == 0.0


def test_zero_stake_with_nonpass_decision_and_nonnull_line_still_grades_no_position():
    # decision not PASS, recommended_stake_dollars absent/0, non-null line
    # present — must still grade as no-position (Push, pnl 0.0), not a
    # booked bet sized off the line value.
    dec = {"contract_side": "YES", "decision": "BUY", "recommended_stake_dollars": 0.0}
    row = {"line": 235.5}
    stake = resolve_row_stake(dec, row)
    assert stake == 0.0
    assert is_no_position(dec["contract_side"], dec["decision"], stake=stake) is True
