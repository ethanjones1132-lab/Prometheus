# SPDX-License-Identifier: AGPL-3.0-or-later
"""Unit tests for technical agent math — pure, no network."""

import math

from agents.technical import annualized_realized_vol, binary_call_prob, clamp_prob
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
