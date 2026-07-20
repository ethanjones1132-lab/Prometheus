"""Unit tests for scripts/grading_common.py — the single shared source of
truth for PASS-guard detection, stake resolution, Kalshi contract PnL, and
side-aware closing-line value (CLV).

These tests exist because an audit found _grade_latest.py and
_grade_pending_kalshi.py disagreed on this logic, and the disagreement
fabricated a fake winning trade for a row the engine had actually declined
to trade (contract_side="NO", decision="PASS", recommended_stake_dollars=0.0).
"""
from __future__ import annotations

import pytest

from grading_common import (
    compute_clv,
    contract_pnl,
    is_no_position,
    is_placeholder_ticker,
    normalize_result,
    resolve_entry_price,
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
# contract_pnl — fee model: fee/contract = fee_mult * p * (1-p), contracts = stake/entry
#
# Hardcoded worked values, not re-derived from the formula under test: if
# this re-derived `contracts - stake - fee` inline, the test would agree
# with a shared misconception in the implementation and would pin no
# magnitude. This is the same worked example as the Rust sibling test
# (kalshi-monster/src-tauri/src/kalshi/grading.rs, contract_pnl_subtracts_
# fees_on_win / _on_loss) — a cross-language pin so Python and Rust can't
# silently drift apart on the fee math. fee_mult is REQUIRED (no default,
# see contract_pnl's docstring), so it's passed explicitly everywhere,
# including here.
# ---------------------------------------------------------------------------

def test_contract_pnl_zero_stake_is_zero_regardless_of_outcome():
    assert contract_pnl(0.0, 0.12, won=True, fee_mult=0.07) == 0.0
    assert contract_pnl(0.0, 0.12, won=False, fee_mult=0.07) == 0.0


def test_contract_pnl_winning_bet_worked_example():
    # $100 stake at 50 cents -> 200 contracts. Gross win = $200 - $100 = $100.
    # Fee = 0.07 * 0.50 * 0.50 * 200 = $3.50. Net PnL = $96.50.
    pnl = contract_pnl(100.0, 0.50, won=True, fee_mult=0.07)
    assert abs(pnl - 96.50) < 1e-9, f"expected 96.50, got {pnl}"


def test_contract_pnl_losing_bet_worked_example():
    # $100 stake at 50 cents -> 200 contracts. Fee = $3.50. Total loss = -$103.50.
    pnl = contract_pnl(100.0, 0.50, won=False, fee_mult=0.07)
    assert abs(pnl - (-103.50)) < 1e-9, f"expected -103.50, got {pnl}"


def test_contract_pnl_fee_mult_is_required_no_silent_default():
    with pytest.raises(TypeError):
        contract_pnl(100.0, 0.50, won=True)  # missing fee_mult


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
    pnl = 0.0 if is_no_position(side, decision_field, stake=stake) else contract_pnl(stake, entry, won=True, fee_mult=0.07)
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

    pnl = contract_pnl(stake, entry, won=True, fee_mult=0.07)
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

    pnl = contract_pnl(stake, entry, won=True, fee_mult=0.07)
    clv = compute_clv(side, entry, close)

    assert pnl > 0
    assert clv == (1.0 - close) - entry


def test_zero_or_absent_stake_never_produces_nonzero_pnl():
    for raw_stake in (0.0, None, -1.0, "junk"):
        stake = resolve_stake(raw_stake)
        assert contract_pnl(stake, 0.5, won=True, fee_mult=0.07) == 0.0
        assert contract_pnl(stake, 0.5, won=False, fee_mult=0.07) == 0.0


# ---------------------------------------------------------------------------
# resolve_entry_price — precedence + side-aware clamp order (I1)
# ---------------------------------------------------------------------------

def test_entry_price_prefers_price_to_enter_decimal():
    decision = {"price_to_enter": 0.35, "market_price_pct": 88.0}
    assert resolve_entry_price(decision, "YES", row_entry_price=0.9) == 0.35


def test_entry_price_price_to_enter_as_percent_is_converted():
    decision = {"price_to_enter": 35.0}
    assert resolve_entry_price(decision, "YES") == 0.35


def test_entry_price_market_price_pct_yes_side():
    decision = {"market_price_pct": 88.0}
    assert resolve_entry_price(decision, "YES") == 0.88


def test_entry_price_market_price_pct_no_side_is_one_minus_yes():
    decision = {"market_price_pct": 88.0}
    assert resolve_entry_price(decision, "NO") == pytest.approx(0.12)


def test_entry_price_market_price_pct_clamped_at_extremes():
    # yes=1.0 -> NO side would be 0.0, clamped up to 0.01.
    assert resolve_entry_price({"market_price_pct": 100.0}, "NO") == 0.01
    # yes=0.0 -> YES side clamped up to 0.01; NO side clamped down to 0.99.
    assert resolve_entry_price({"market_price_pct": 0.0}, "YES") == 0.01
    assert resolve_entry_price({"market_price_pct": 0.0}, "NO") == 0.99


def test_entry_price_falls_back_to_row_entry_price_when_decision_has_neither_field():
    decision = {"contract_side": "YES"}  # no price_to_enter, no market_price_pct
    assert resolve_entry_price(decision, "YES", row_entry_price=0.42) == 0.42


def test_entry_price_falls_back_to_neutral_default_when_nothing_available():
    assert resolve_entry_price(None, "YES", row_entry_price=None) == 0.5
    assert resolve_entry_price({}, "YES", row_entry_price=None) == 0.5


# ---------------------------------------------------------------------------
# is_placeholder_ticker — unified across both scripts (I5, partial)
# ---------------------------------------------------------------------------

def test_placeholder_ticker_exact_matches():
    assert is_placeholder_ticker("KXEVENT-TICKER") is True
    assert is_placeholder_ticker("TICKER") is True
    assert is_placeholder_ticker("") is True
    assert is_placeholder_ticker(None) is True


def test_placeholder_ticker_suffix_and_keyword_matches():
    assert is_placeholder_ticker("KX-FOO-TICKER") is True
    assert is_placeholder_ticker("KXFOO-PLACEHOLDER-1") is True
    assert is_placeholder_ticker("KXFOO-EXAMPLE-1") is True


def test_placeholder_ticker_real_ticker_is_not_placeholder():
    assert is_placeholder_ticker("KXNBASUMMERSPREAD-26JUL13MINPOR-POR16") is False
    assert is_placeholder_ticker("KXMLBTEAMTOTAL-26JUL171915CWSTOR-TOR8") is False


# ---------------------------------------------------------------------------
# normalize_result — unified across both scripts (I5, partial)
# ---------------------------------------------------------------------------

def test_normalize_result_accepts_common_yes_no_spellings():
    assert normalize_result("yes") == "Yes"
    assert normalize_result("Y") == "Yes"
    assert normalize_result("true") == "Yes"
    assert normalize_result("1") == "Yes"
    assert normalize_result("no") == "No"
    assert normalize_result("N") == "No"
    assert normalize_result("false") == "No"
    assert normalize_result("0") == "No"


def test_normalize_result_unrecognized_or_empty_is_none():
    assert normalize_result("") is None
    assert normalize_result(None) is None
    assert normalize_result("maybe") is None
