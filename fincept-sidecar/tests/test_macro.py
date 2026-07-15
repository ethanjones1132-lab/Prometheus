# SPDX-License-Identifier: AGPL-3.0-or-later
"""Sprint 4 — macro agent mapping + null paths (no network)."""

from datetime import datetime, timezone, timedelta

from agents.macro import (
    binary_from_level,
    estimate_sync,
    infer_macro_mapping,
    infer_threshold,
    realized_level,
    yoy_pct,
)
from fincept_sidecar.schemas import MarketCategory, MarketOpinionRequest


def _req(**kwargs) -> MarketOpinionRequest:
    base = dict(
        market_ticker="KXTEST",
        title="Test",
        resolution_rules="Resolves Yes/No",
        close_time=datetime.now(timezone.utc) + timedelta(days=14),
        category=MarketCategory.ECONOMIC,
        yes_bid=0.40,
        yes_ask=0.42,
        context={},
    )
    base.update(kwargs)
    return MarketOpinionRequest(**base)


def test_maps_cpi_series_from_ticker():
    m = infer_macro_mapping(
        _req(market_ticker="KXCPIYOY-26", title="Will CPI YoY be above 3%?")
    )
    assert m is not None
    assert m.fred_id == "CPIAUCSL"
    assert m.transform == "yoy"


def test_maps_unemployment():
    m = infer_macro_mapping(
        _req(title="Will the unemployment rate be below 4.5%?")
    )
    assert m is not None
    assert m.fred_id == "UNRATE"


def test_unmapped_returns_null():
    sig = estimate_sync(
        _req(
            market_ticker="KXWEATHER-1",
            title="Will it rain in NYC?",
            category=MarketCategory.OTHER,
        )
    )
    assert sig.probability is None
    assert "no_series_mapping" in sig.caveats


def test_mapped_but_no_threshold_null():
    sig = estimate_sync(
        _req(
            market_ticker="KXCPI-26",
            title="Will CPI surprise markets this month?",
            category=MarketCategory.ECONOMIC,
        ),
        observations=[100.0, 101.0, 102.0] * 5,
    )
    assert sig.probability is None
    assert "missing:threshold" in sig.caveats


def test_missing_fred_key_null_when_no_observations(monkeypatch):
    monkeypatch.delenv("FRED_API_KEY", raising=False)
    monkeypatch.delenv("fred_api_key", raising=False)
    sig = estimate_sync(
        _req(
            market_ticker="KXCPIYOY-26",
            title="Will CPI YoY be above 3.0%?",
            category=MarketCategory.ECONOMIC,
            context={},  # no key
        ),
        observations=None,
    )
    assert sig.probability is None
    assert "missing:fred_api_key" in sig.caveats


def test_opines_with_injected_series():
    # Synthetic CPI levels with ~3% YoY
    levels = [100.0 + i * 0.25 for i in range(24)]
    # force last yoy ~ known
    y = yoy_pct(levels, 12)
    assert y is not None
    sig = estimate_sync(
        _req(
            market_ticker="KXCPIYOY-26",
            title="Will CPI YoY print above 2.0%?",
            category=MarketCategory.ECONOMIC,
        ),
        observations=levels,
    )
    assert sig.probability is not None
    assert 0.01 <= sig.probability <= 0.99
    assert sig.confidence > 0


def test_threshold_and_binary_below():
    thr = infer_threshold("Will unemployment be below 4.0%?")
    assert thr is not None
    assert thr.direction == "below"
    assert thr.value == 4.0
    # Level well below threshold → high P(YES) for below
    p = binary_from_level(3.5, thr, sigma=0.3)
    assert p > 0.7


def test_realized_yoy():
    vals = [100.0] * 12 + [103.0]
    assert abs(realized_level(vals, "yoy") - 3.0) < 0.01
