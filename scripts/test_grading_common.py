"""Unit tests for scripts/grading_common.py — the single shared source of
truth for PASS-guard detection, stake resolution, Kalshi contract PnL, and
side-aware closing-line value (CLV).

These tests exist because an audit found _grade_latest.py and
_grade_pending_kalshi.py disagreed on this logic, and the disagreement
fabricated a fake winning trade for a row the engine had actually declined
to trade (contract_side="NO", decision="PASS", recommended_stake_dollars=0.0).
"""
from __future__ import annotations

from grading_common import (
    compute_clv,
    contract_pnl,
    is_no_position,
    resolve_stake,
)


# ---------------------------------------------------------------------------
# is_no_position
# ---------------------------------------------------------------------------

def test_no_position_when_side_and_decision_absent():
    assert is_no_position(None, None, stake=0.0) is True


def test_no_position_when_side_is_pass():
    assert is_no_position("PASS", None, stake=10.0) is True


def test_no_position_when_decision_is_pass_even_if_side_names_no():
    # The exact real-world bug: contract_side="NO" but decision="PASS".
    # The engine named a side while still declining to trade.
    assert is_no_position("NO", "PASS", stake=10.0) is True


def test_no_position_when_decision_is_pass_even_if_side_names_yes():
    assert is_no_position("YES", "PASS", stake=10.0) is True


def test_no_position_when_stake_is_zero_even_with_real_side_and_decision():
    assert is_no_position("YES", "BUY", stake=0.0) is True


def test_no_position_when_stake_is_negative():
    assert is_no_position("YES", "BUY", stake=-5.0) is True


def test_not_no_position_for_genuine_yes_bet():
    assert is_no_position("YES", "BUY", stake=10.0) is False


def test_not_no_position_for_genuine_no_bet():
    assert is_no_position("NO", "BUY", stake=10.0) is False


# ---------------------------------------------------------------------------
# resolve_stake — no phantom $10 default
# ---------------------------------------------------------------------------

def test_resolve_stake_zero_stays_zero():
    assert resolve_stake(0.0) == 0.0


def test_resolve_stake_absent_stays_zero():
    assert resolve_stake(None) == 0.0


def test_resolve_stake_negative_stays_zero():
    assert resolve_stake(-3.0) == 0.0


def test_resolve_stake_non_numeric_stays_zero():
    assert resolve_stake("not a number") == 0.0


def test_resolve_stake_positive_value_passes_through():
    assert resolve_stake(25.0) == 25.0


# ---------------------------------------------------------------------------
# contract_pnl — fee model: fee/contract = 0.07 * p * (1-p), contracts = stake/entry
# ---------------------------------------------------------------------------

def test_contract_pnl_zero_stake_is_zero_regardless_of_outcome():
    assert contract_pnl(0.0, 0.12, won=True) == 0.0
    assert contract_pnl(0.0, 0.12, won=False) == 0.0


def test_contract_pnl_winning_yes_bet_is_positive():
    stake, entry = 10.0, 0.40
    pnl = contract_pnl(stake, entry, won=True)
    contracts = stake / entry
    fee = 0.07 * entry * (1.0 - entry) * contracts
    assert pnl == contracts - stake - fee
    assert pnl > 0


def test_contract_pnl_losing_bet_is_negative_stake_plus_fee():
    stake, entry = 10.0, 0.40
    pnl = contract_pnl(stake, entry, won=False)
    contracts = stake / entry
    fee = 0.07 * entry * (1.0 - entry) * contracts
    assert pnl == -(stake + fee)
    assert pnl < 0


# ---------------------------------------------------------------------------
# compute_clv — side-aware
# ---------------------------------------------------------------------------

def test_clv_yes_side_is_close_minus_entry():
    assert compute_clv("YES", entry=0.30, close=1.0) == 1.0 - 0.30


def test_clv_no_side_is_one_minus_close_minus_entry():
    assert compute_clv("NO", entry=0.12, close=1.0) == (1.0 - 1.0) - 0.12


def test_clv_no_side_when_market_resolves_no():
    assert compute_clv("NO", entry=0.12, close=0.0) == (1.0 - 0.0) - 0.12


def test_clv_pass_or_unknown_side_is_zero():
    assert compute_clv("PASS", entry=0.12, close=1.0) == 0.0
    assert compute_clv(None, entry=0.12, close=1.0) == 0.0


# ---------------------------------------------------------------------------
# The exact real-world fabricated row (id=9e6309fb-73f7-4bb8-85a6-5f7cfb884ab5,
# ticker=KXMLBTEAMTOTAL-26JUL171915CWSTOR-TOR8) — end-to-end through the
# helpers the way a grading script combines them.
# ---------------------------------------------------------------------------

def test_real_world_pass_row_grades_to_push_zero_pnl_zero_clv():
    decision_json = {
        "ticker": "KXMLBTEAMTOTAL-26JUL171915CWSTOR-TOR8",
        "contract_side": "NO",
        "decision": "PASS",
        "price_to_enter": 0.12,
        "recommended_stake_dollars": 0.0,
        "market_price_pct": 88.0,
        "fair_probability_pct": 22.0,
    }
    side = decision_json["contract_side"]
    decision_field = decision_json["decision"]
    stake = resolve_stake(decision_json.get("recommended_stake_dollars"))
    entry = decision_json["price_to_enter"]

    assert is_no_position(side, decision_field, stake=stake) is True

    # A grading script, on seeing is_no_position() is True, must not call
    # contract_pnl/compute_clv with the "real bet" branch at all — but even
    # if it mistakenly did, stake=0 forces pnl=0, and a correctly-guarded
    # script reports clv=0.0 for no-position rows regardless of `side`.
    pnl = 0.0 if is_no_position(side, decision_field, stake=stake) else contract_pnl(stake, entry, won=True)
    clv = 0.0 if is_no_position(side, decision_field, stake=stake) else compute_clv(side, entry, close=1.0)

    assert pnl == 0.0
    assert clv == 0.0


def test_genuine_yes_bet_that_wins_has_positive_pnl_and_matching_clv():
    decision_json = {
        "contract_side": "YES",
        "decision": "BUY",
        "price_to_enter": 0.35,
        "recommended_stake_dollars": 20.0,
    }
    side = decision_json["contract_side"]
    stake = resolve_stake(decision_json["recommended_stake_dollars"])
    entry = decision_json["price_to_enter"]
    close = 1.0  # market resolved Yes

    assert is_no_position(side, decision_json["decision"], stake=stake) is False

    pnl = contract_pnl(stake, entry, won=True)
    clv = compute_clv(side, entry, close)

    assert pnl > 0
    assert clv == close - entry


def test_genuine_no_bet_that_wins_has_positive_pnl_and_matching_clv():
    decision_json = {
        "contract_side": "NO",
        "decision": "BUY",
        "price_to_enter": 0.20,
        "recommended_stake_dollars": 15.0,
    }
    side = decision_json["contract_side"]
    stake = resolve_stake(decision_json["recommended_stake_dollars"])
    entry = decision_json["price_to_enter"]
    close = 0.0  # market resolved No -> NO side wins

    assert is_no_position(side, decision_json["decision"], stake=stake) is False

    pnl = contract_pnl(stake, entry, won=True)
    clv = compute_clv(side, entry, close)

    assert pnl > 0
    assert clv == (1.0 - close) - entry


def test_zero_or_absent_stake_never_produces_nonzero_pnl():
    for raw_stake in (0.0, None, -1.0, "junk"):
        stake = resolve_stake(raw_stake)
        assert contract_pnl(stake, 0.5, won=True) == 0.0
        assert contract_pnl(stake, 0.5, won=False) == 0.0
