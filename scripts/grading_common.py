#!/usr/bin/env python3
"""Shared grading logic for the Kalshi prediction grading scripts.

_grade_latest.py and _grade_pending_kalshi.py both read the same
`full_decision_json` shape and must agree on:

  1. whether a decision represents a real position or a declined trade
     ("PASS"), including the case where the engine names a side
     (contract_side="YES"/"NO") while `decision` itself is "PASS" —
     the engine can identify which side it *would* take without
     actually recommending the trade;
  2. how a dollar stake is resolved (no phantom nominal stake — an
     absent/zero/negative recommended_stake_dollars means no position,
     not a fallback bet);
  3. the Kalshi fee model and contract PnL math; and
  4. side-aware closing-line value (CLV).

An earlier audit found these two scripts had drifted apart on exactly this
logic, and the drift fabricated a fake winning trade for a row the engine
had actually declined to trade. This module is the single source of truth
so that can't happen again.
"""
from __future__ import annotations

from numbers import Real


def is_no_position(side: str | None, decision: str | None, stake: float | None = None) -> bool:
    """True when the row represents no real position taken.

    A row is "no position" if any of the following hold:
      - the parsed contract side is missing/unknown or literally "PASS"
      - the `decision` field says "PASS", *regardless* of what
        contract_side says (this is the case that was previously
        mis-graded: contract_side="NO", decision="PASS")
      - the resolved stake is <= 0
    """
    side_u = (side or "").strip().upper()
    decision_u = (decision or "").strip().upper()
    if side_u in ("", "PASS"):
        return True
    if decision_u == "PASS":
        return True
    if stake is not None and stake <= 0:
        return True
    return False


def resolve_stake(recommended_stake_dollars) -> float:
    """Resolve the dollar stake for a decision.

    No phantom fallback: an absent, non-numeric, zero, or negative
    recommended_stake_dollars resolves to 0.0 (no position) — never a
    nominal placeholder stake.
    """
    if isinstance(recommended_stake_dollars, Real) and float(recommended_stake_dollars) > 0:
        return float(recommended_stake_dollars)
    return 0.0


def contract_pnl(stake: float, entry: float, won: bool, fee_mult: float = 0.07) -> float:
    """Kalshi contract PnL, net of fees.

    fee per contract = fee_mult * p * (1 - p)
    contracts         = stake / entry
    win pnl           = contracts - stake - fee
    loss pnl          = -(stake + fee)
    """
    if stake <= 0:
        return 0.0
    p = max(0.01, min(0.99, entry))
    contracts = stake / p
    fee = fee_mult * p * (1.0 - p) * contracts
    if not won:
        return -(stake + fee)
    return contracts - stake - fee


def compute_clv(side: str | None, entry: float, close: float) -> float:
    """Side-aware closing-line value.

      YES side: clv = close - entry
      NO side:  clv = (1.0 - close) - entry
      anything else (PASS / no position / unknown): clv = 0.0
    """
    side_u = (side or "").strip().upper()
    if side_u == "YES":
        return close - entry
    if side_u == "NO":
        return (1.0 - close) - entry
    return 0.0
