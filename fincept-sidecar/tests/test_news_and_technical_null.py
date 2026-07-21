# SPDX-License-Identifier: AGPL-3.0-or-later
"""Sprint 1.4 — technical + news null paths (no network)."""

from datetime import datetime, timezone, timedelta

from agents.news import estimate_sync as news_estimate
from agents.technical import infer_strike, infer_underlying_ticker, years_to_close
from fincept_sidecar.schemas import AgentSignal, MarketCategory, MarketOpinionRequest


def _req(**kwargs) -> MarketOpinionRequest:
    base = dict(
        market_ticker="KXTEST",
        title="Test market",
        resolution_rules="Resolves to Yes or No",
        close_time=datetime.now(timezone.utc) + timedelta(days=7),
        category=MarketCategory.POLITICAL,
        yes_bid=0.45,
        yes_ask=0.47,
        context={},
    )
    base.update(kwargs)
    return MarketOpinionRequest(**base)


def test_news_null_without_snippets():
    sig = news_estimate(_req())
    assert sig.agent == "news"
    assert sig.probability is None
    assert "missing:web_snippets" in sig.caveats


def test_news_null_when_snippets_inconclusive():
    sig = news_estimate(
        _req(
            context={
                "web_snippets": [
                    {
                        "title": "Market update",
                        "url": "https://example.com/a",
                        "snippet": "Analysts discuss various outcomes without a clear winner.",
                    }
                ]
            }
        )
    )
    assert sig.probability is None
    assert "evidence_inconclusive" in sig.caveats


def test_news_opines_on_strong_yes_language():
    sig = news_estimate(
        _req(
            yes_bid=0.50,
            yes_ask=0.52,
            context={
                "web_snippets": [
                    {
                        "title": "Candidate clinches primary victory",
                        "url": "https://example.com/win",
                        "snippet": "She wins the race and secures the nomination after leading polls.",
                    }
                ]
            },
        )
    )
    assert sig.probability is not None
    assert sig.probability > 0.50
    assert 0.0 < sig.confidence <= 0.35


def test_technical_null_when_missing_underlying_and_strike():
    req = _req(
        category=MarketCategory.INDEX_PRICE_LEVEL,
        title="Will this contract resolve yes?",
        market_ticker="KXZZZZ-99",
        context={},
    )
    yf = infer_underlying_ticker(req)
    strike = infer_strike(req)
    tau = years_to_close(req.close_time)
    # Without series tokens or barrier, technical cannot opine
    assert yf is None
    assert strike is None
    assert tau is not None  # close_time still yields horizon
    # Full early-exit shape
    missing = []
    if yf is None:
        missing.append("underlying")
    if strike is None:
        missing.append("strike")
    assert "underlying" in missing and "strike" in missing
    sig = AgentSignal(
        agent="technical",
        probability=None,
        confidence=0.0,
        rationale="missing " + ",".join(missing),
        inputs_used=[],
        caveats=[f"missing:{m}" for m in missing],
    )
    assert sig.probability is None


def test_technical_infers_btc_series_and_barrier_strike():
    req = _req(
        market_ticker="KXBTCD-26JUL15-B100000",
        title="Bitcoin price range",
        category=MarketCategory.INDEX_PRICE_LEVEL,
        context={},
    )
    assert infer_underlying_ticker(req) == "BTC-USD"
    # B-legs are brackets; representative strike is the bin mid (~center).
    assert abs(infer_strike(req) - 100_000.0) < 0.1
    from agents.technical import infer_contract_spec

    spec = infer_contract_spec(req)
    assert spec is not None
    assert spec["style"] == "bracket"
    assert abs(spec["floor"] - 99_950.0) < 1.0
    assert abs(spec["cap"] - 100_049.99) < 1.0


def test_horizon_days_from_context_preferred():
    tau = years_to_close(
        datetime.now(timezone.utc) + timedelta(days=30),
        horizon_days=3.0,
    )
    assert tau is not None
    assert abs(tau - 3.0 / 365.25) < 1e-9


def test_quick_depth_skips_technical_news_macro():
    """Sprint 3.1 — board scan depth only runs contract_tape for live signal."""
    import asyncio
    from agents.orchestrator import collect_market_opinion

    req = _req(
        category=MarketCategory.INDEX_PRICE_LEVEL,
        context={"depth": "quick", "contract_mids": [0.4, 0.42, 0.45, 0.48, 0.5]},
    )
    resp = asyncio.run(collect_market_opinion(req))
    by_name = {s.agent: s for s in resp.signals}
    assert by_name["technical"].probability is None
    assert "depth=quick" in by_name["technical"].rationale or "depth_skipped" in by_name[
        "technical"
    ].caveats
    assert by_name["news"].probability is None
    assert by_name["macro"].probability is None
    assert by_name["contract_tape"].probability is not None
