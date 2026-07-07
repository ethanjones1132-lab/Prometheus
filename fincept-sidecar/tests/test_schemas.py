# SPDX-License-Identifier: AGPL-3.0-or-later
"""Wire-contract tests (plan §5, §11): these schemas are what the Rust
edge_engine deserializes — breakage must fail here first."""

import pytest
from pydantic import ValidationError

from fincept_sidecar.schemas import (
    AgentSignal,
    AssetSignal,
    MarketCategory,
    MarketOpinionRequest,
)


def test_agent_signal_accepts_no_opinion():
    s = AgentSignal(agent="sentiment", probability=None, confidence=0.5, rationale="no relevant data")
    assert s.probability is None
    assert s.inputs_used == [] and s.caveats == []


def test_agent_signal_probability_bounds_enforced():
    # [0.01, 0.99] per plan §5 / Appendix C — reject overconfident extremes.
    for bad in (0.0, 0.005, 0.995, 1.0, -0.2, 1.7):
        with pytest.raises(ValidationError):
            AgentSignal(agent="macro", probability=bad, confidence=0.5, rationale="x")
    AgentSignal(agent="macro", probability=0.01, confidence=0.5, rationale="x")
    AgentSignal(agent="macro", probability=0.99, confidence=0.5, rationale="x")


def test_agent_signal_confidence_bounds():
    with pytest.raises(ValidationError):
        AgentSignal(agent="macro", probability=0.5, confidence=1.5, rationale="x")
    with pytest.raises(ValidationError):
        AgentSignal(agent="macro", probability=0.5, confidence=-0.1, rationale="x")


def test_market_opinion_request_parses_worked_example():
    # The §4.4 CPI example as a wire payload.
    req = MarketOpinionRequest(
        market_ticker="CPI-26JUL-A3.0",
        title="Will July CPI YoY exceed 3.0%?",
        resolution_rules="Resolves YES if BLS CPI-U YoY (one decimal) exceeds 3.0% at the Aug 12 release.",
        close_time="2026-08-12T12:30:00Z",
        category=MarketCategory.ECONOMIC,
        yes_bid=0.70,
        yes_ask=0.72,
    )
    assert req.category is MarketCategory.ECONOMIC
    assert req.context == {}


def test_market_opinion_request_rejects_out_of_range_prices():
    with pytest.raises(ValidationError):
        MarketOpinionRequest(
            market_ticker="X",
            title="x",
            resolution_rules="x",
            close_time="2026-08-12T12:30:00Z",
            category=MarketCategory.OTHER,
            yes_bid=-0.1,
            yes_ask=1.2,
        )


def test_asset_signal_forward_contract():
    # §14.4: crypto valuation agents must be able to say "no opinion".
    s = AssetSignal(
        agent="valuation",
        ticker="BTC-USD",
        horizon_days=21,
        expected_excess_return=None,
        return_vol=0.60,
        confidence=0.0,
        rationale="no cash flows to discount",
    )
    assert s.expected_excess_return is None
    with pytest.raises(ValidationError):
        AssetSignal(
            agent="technical",
            ticker="BTC-USD",
            horizon_days=0,  # must be > 0
            return_vol=0.6,
            confidence=0.5,
            rationale="x",
        )


def test_rust_mirror_fixture_round_trips():
    """The exact JSON the Rust test-suite uses must validate here too —
    keeps the two AgentSignal definitions in lock-step."""
    fixture = {
        "agent": "macro",
        "probability": 0.91,
        "confidence": 0.85,
        "rationale": "nowcast 3.2% with sigma 0.15",
        "inputs_used": [{"source": "econdb:CPI-US", "fetched_at": "2026-07-07T12:00:00Z"}],
        "caveats": ["base effects fading"],
    }
    s = AgentSignal.model_validate(fixture)
    assert s.agent == "macro"
    assert abs(s.probability - 0.91) < 1e-12
