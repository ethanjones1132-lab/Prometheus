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
  3. how a decimal entry price is resolved, side-aware;
  4. the Kalshi fee model and contract PnL math;
  5. side-aware closing-line value (CLV); and
  6. which tickers are schema placeholders that must never be graded.

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

    Deliberately does NOT fall back to any other DB column (e.g. the
    `line` column, which is overloaded — it stores a sports-prop line for
    non-Kalshi rows, not a dollar amount). recommended_stake_dollars is
    the only source of truth for stake.
    """
    if isinstance(recommended_stake_dollars, Real) and float(recommended_stake_dollars) > 0:
        return float(recommended_stake_dollars)
    return 0.0


def resolve_entry_price(
    decision: dict | None,
    side: str | None,
    row_entry_price: float | None = None,
) -> float:
    """Resolve the decimal (0..1) entry price for a decision, side-aware.

    Precedence, fixed here deliberately so both grading scripts agree on
    both the fallback order AND the clamp order (they previously computed
    the NO-side price in different sequences — clamp-then-subtract vs.
    subtract-then-clamp — and only ever agreed numerically because the
    [0.01, 0.99] clamp bounds happen to be symmetric around 0.5):

      1. decision['price_to_enter'], taken as-is (already the specific
         price the engine says it would enter this contract at) — not
         re-clamped or side-adjusted, and not further validated beyond
         "is it a plausible 0..1 or 1..100 price".
      2. decision['market_price_pct'], converted to a YES decimal, then
         to the resolved side's price (NO = 1 - yes), THEN clamped to
         [0.01, 0.99] as the last step — this avoids a near-zero
         denominator in `contracts = stake / entry` without silently
         producing a different number than clamping earlier would.
      3. row_entry_price (the row's previously-recorded entry_price
         column), if within (0, 1] — used when full_decision_json didn't
         carry either of the above.
      4. 0.5, a neutral last resort so PnL math never divides by
         zero/near-zero.
    """
    if decision:
        pte = decision.get("price_to_enter")
        if isinstance(pte, Real) and 0 < float(pte) <= 1:
            return float(pte)
        if isinstance(pte, Real) and 1 < float(pte) <= 100:
            return float(pte) / 100.0
        mkt = decision.get("market_price_pct")
        if isinstance(mkt, Real):
            yes = float(mkt) / 100.0 if float(mkt) > 1 else float(mkt)
            side_u = (side or "").strip().upper()
            value = (1.0 - yes) if side_u == "NO" else yes
            return max(0.01, min(0.99, value))
    if row_entry_price is not None and 0 < float(row_entry_price) <= 1:
        return float(row_entry_price)
    return 0.5


def contract_pnl(stake: float, entry: float, won: bool, fee_mult: float) -> float:
    """Kalshi contract PnL, net of fees.

    fee per contract = fee_mult * p * (1 - p)
    contracts         = stake / entry
    win pnl           = contracts - stake - fee
    loss pnl          = -(stake + fee)

    `fee_mult` is required (no default) on purpose: the Rust grader
    (kalshi-monster/src-tauri/src/kalshi/grading.rs::contract_pnl) threads
    its fee multiplier from `edge_cfg.fee_multiplier` rather than baking
    in a constant. A Python-side default here could silently drift from
    that config-driven value without anyone noticing. Callers must pass
    it explicitly (currently 0.07, matching the Rust default) so any
    future divergence is a visible, reviewable diff instead of an
    implicit one.
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

    Sibling implementation: kalshi-monster/src-tauri/src/predictions/
    storage.rs::update_prediction_clv computes the same formula for the
    same `predictions.clv` column when the Tauri app grades a row directly
    (rather than via these cron scripts). A prior version of that Rust
    function was side-blind (`close - entry` unconditionally) and
    disagreed with this function on every NO-side row. If either
    implementation's formula changes, update the other to match.
    """
    side_u = (side or "").strip().upper()
    if side_u == "YES":
        return close - entry
    if side_u == "NO":
        return (1.0 - close) - entry
    return 0.0


_PLACEHOLDER_TICKER_EXACT = {
    "KXEVENT-TICKER",
    "KX-EVENT-TICKER",
    "KXEVENT",
    "TICKER",
    "KX-TICKER",
    "KXTEST",
}


def is_placeholder_ticker(ticker: str | None) -> bool:
    """True for schema-placeholder tickers that must never be graded.

    Mirrors KalshiTradeDecision::is_placeholder_ticker in
    kalshi-monster/src-tauri/src/chat/decision_schema.rs — the two Python
    grading scripts each had their own, different, narrower placeholder
    predicate (one required "TICKER" AND "EVENT" both present; the other
    required "TICKER" present AND length < 20). Neither matched the
    definition the rest of the app already enforces. Unified here to that
    canonical definition instead of inventing a third variant.
    """
    t = (ticker or "").strip().upper()
    if not t:
        return True
    if t in _PLACEHOLDER_TICKER_EXACT:
        return True
    if t.endswith("-TICKER"):
        return True
    if "PLACEHOLDER" in t or "EXAMPLE" in t:
        return True
    return False


def normalize_result(raw: str | None) -> str | None:
    """Normalize a Kalshi market settlement result string to 'Yes' / 'No'.

    Was byte-identical, independently maintained, in both grading
    scripts; unified here so a future tweak to accepted spellings can't
    apply to only one of them.
    """
    t = (raw or "").strip().lower()
    if t in ("yes", "y", "true", "1"):
        return "Yes"
    if t in ("no", "n", "false", "0"):
        return "No"
    return None
