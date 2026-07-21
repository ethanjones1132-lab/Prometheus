# SPDX-License-Identifier: AGPL-3.0-or-later
"""Unit tests for technical agent math — pure, no network."""

from agents.technical import (
    annualized_realized_vol,
    binary_bracket_prob,
    binary_call_prob,
    binary_put_prob,
    clamp_prob,
    infer_contract_spec,
    price_contract_prob,
)
from agents.contract_tape import estimate_sync
from fincept_sidecar.schemas import MarketCategory, MarketOpinionRequest
from datetime import datetime, timezone, timedelta


def test_binary_call_atm_short_horizon_near_half():
    # ATM, tiny time, any vol → ≈ 0.5
    p = binary_call_prob(spot=100.0, strike=100.0, sigma=0.20, tau_years=1 / 365.0, mu=0.0)
    assert abs(p - 0.5) < 0.03


def test_binary_call_deep_itm_high_prob():
    p = binary_call_prob(spot=120.0, strike=100.0, sigma=0.15, tau_years=30 / 365.0, mu=0.0)
    assert p > 0.9


def test_binary_call_deep_otm_low_prob():
    p = binary_call_prob(spot=80.0, strike=100.0, sigma=0.15, tau_years=30 / 365.0, mu=0.0)
    assert p < 0.1


def test_binary_put_complements_call():
    call = binary_call_prob(100.0, 100.0, 0.2, 30 / 365.0)
    put = binary_put_prob(100.0, 100.0, 0.2, 30 / 365.0)
    assert abs(call + put - 1.0) < 1e-12


def test_bracket_prob_less_than_call_when_spot_above_bin():
    """Regression: B-legs must NOT be priced as P(S>K).

    Spot well above a lower bin → call≈1, but bracket mass is tiny.
    """
    spot, floor, cap = 66_000.0, 65_000.0, 65_099.99
    sigma, tau = 0.50, 2 / (365.25 * 24)  # ~2 hours
    call = binary_call_prob(spot, floor, sigma, tau)
    br = binary_bracket_prob(spot, floor, cap, sigma, tau)
    assert call > 0.9
    assert br < 0.15
    assert br < call


def test_bracket_prob_peaks_near_spot():
    spot, sigma, tau = 100.0, 0.20, 7 / 365.0
    near = binary_bracket_prob(spot, 99.0, 101.0, sigma, tau)
    far = binary_bracket_prob(spot, 120.0, 122.0, sigma, tau)
    assert near > far


def test_infer_b_ticker_is_bracket():
    req = MarketOpinionRequest(
        market_ticker="KXBTC-26JUL2114-B73250",
        title="Bitcoin price range",
        resolution_rules="between 73200-73299.99",
        close_time=datetime.now(timezone.utc) + timedelta(hours=1),
        category=MarketCategory.INDEX_PRICE_LEVEL,
        yes_bid=0.05,
        yes_ask=0.07,
        context={},
    )
    spec = infer_contract_spec(req)
    assert spec is not None
    assert spec["style"] == "bracket"
    assert abs(spec["floor"] - 73200.0) < 1.0
    assert abs(spec["cap"] - 73299.99) < 1.0


def test_infer_api_bounds_above_and_below():
    base = dict(
        market_ticker="KXINX-TEST-T1",
        title="SPX",
        resolution_rules="x",
        close_time=datetime.now(timezone.utc) + timedelta(hours=4),
        category=MarketCategory.INDEX_PRICE_LEVEL,
        yes_bid=0.1,
        yes_ask=0.12,
    )
    above = infer_contract_spec(
        MarketOpinionRequest(**base, context={"floor_strike": 5500.0})
    )
    assert above["style"] == "above"
    below = infer_contract_spec(
        MarketOpinionRequest(**base, context={"cap_strike": 5000.0})
    )
    assert below["style"] == "below"


def test_price_contract_prob_uses_style():
    # Above deep ITM ≈ 1; same levels as bracket stay small when far from spot
    p_above = price_contract_prob(
        120.0, {"style": "above", "floor": 100.0, "cap": None}, 0.15, 30 / 365.0
    )
    p_br = price_contract_prob(
        120.0,
        {"style": "bracket", "floor": 100.0, "cap": 101.0},
        0.15,
        30 / 365.0,
    )
    assert p_above > 0.9
    assert p_br < 0.2


def test_realized_vol_from_flat_series_near_zero():
    closes = [100.0] * 30
    # flat → zero vol; annualized_realized_vol returns 0.0
    vol = annualized_realized_vol(closes)
    assert vol is not None
    assert vol < 1e-9


def test_realized_vol_rejects_short_series():
    assert annualized_realized_vol([100.0, 101.0]) is None


def test_clamp_prob_bounds():
    assert clamp_prob(0.0) == 0.01
    assert clamp_prob(1.0) == 0.99
    assert clamp_prob(0.5) == 0.5


def test_contract_tape_none_without_series():
    req = MarketOpinionRequest(
        market_ticker="KXTEST",
        title="Test",
        resolution_rules="Test",
        close_time=datetime.now(timezone.utc) + timedelta(days=7),
        category=MarketCategory.OTHER,
        yes_bid=0.40,
        yes_ask=0.42,
        context={},
    )
    sig = estimate_sync(req)
    assert sig.agent == "contract_tape"
    # single mid only → no opinion (honest)
    assert sig.probability is None


def test_contract_tape_uses_mid_series():
    mids = [0.40, 0.42, 0.45, 0.48, 0.50]
    req = MarketOpinionRequest(
        market_ticker="KXTEST",
        title="Test",
        resolution_rules="Test",
        close_time=datetime.now(timezone.utc) + timedelta(days=7),
        category=MarketCategory.INDEX_PRICE_LEVEL,
        yes_bid=0.49,
        yes_ask=0.51,
        context={"contract_mids": mids},
    )
    sig = estimate_sync(req)
    assert sig.probability is not None
    assert 0.01 <= sig.probability <= 0.99
    assert any("contract_mids" in d.source for d in sig.inputs_used)
