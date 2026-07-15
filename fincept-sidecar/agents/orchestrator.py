# Agent orchestrator — fan-out to available estimators (plan §5, §7 Phase 2)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Depth tiers (Sprint 3 / plan §7):
#   quick    — contract_tape only (board scan; no yfinance / news)
#   standard — technical + contract_tape + news (default Analyze / chat)
#   deep     — same live agents + full history path (technical always runs)
#
# Explicitly NOT shipped yet (no honest data path right now):
#   - macro (needs EconDB / release calendars — not installed)
#   - sentiment (need social/news sentiment feeds)
#   - valuation / fundamentals (need fundamentals DB for company events)

from __future__ import annotations

import time
from datetime import datetime, timezone

from fincept_sidecar.schemas import (
    AgentSignal,
    CatalystEvent,
    MarketOpinionRequest,
    MarketOpinionResponse,
)

from . import contract_tape, news, technical


def _depth(req: MarketOpinionRequest) -> str:
    ctx = req.context or {}
    raw = ctx.get("depth") or "standard"
    if isinstance(raw, str):
        d = raw.strip().lower()
        if d in ("quick", "standard", "deep"):
            return d
    return "standard"


def _null_signal(name: str, reason: str, caveat: str = "depth_skipped") -> AgentSignal:
    return AgentSignal(
        agent=name,
        probability=None,
        confidence=0.0,
        rationale=reason,
        inputs_used=[],
        caveats=[caveat],
    )


async def collect_market_opinion(req: MarketOpinionRequest) -> MarketOpinionResponse:
    t0 = time.perf_counter()
    signals: list[AgentSignal] = []
    depth = _depth(req)

    if depth == "quick":
        # Board scan: tape only — skip yfinance technical + news.
        signals.append(
            _null_signal(
                "technical",
                "depth=quick: technical (yfinance) skipped for board scan latency.",
            )
        )
        signals.append(await contract_tape.estimate(req))
        signals.append(
            _null_signal(
                "news",
                "depth=quick: news agent skipped (no web fetch on board scan).",
            )
        )
    else:
        # standard + deep: full live estimators
        signals.append(await technical.estimate(req))
        signals.append(await contract_tape.estimate(req))
        signals.append(await news.estimate(req))

    # Placeholder no-opinion rows for agents that exist in the plan but have
    # no live data path here — honest None, never a fake probability.
    for name, reason in (
        (
            "macro",
            "Macro agent requires EconDB (or equivalent) release series; not available in this sidecar build."
            if depth != "quick"
            else "depth=quick: macro skipped.",
        ),
        (
            "sentiment",
            "Sentiment agent requires social/news sentiment feeds; not wired."
            if depth != "quick"
            else "depth=quick: sentiment skipped.",
        ),
    ):
        signals.append(
            AgentSignal(
                agent=name,
                probability=None,
                confidence=0.0,
                rationale=reason,
                inputs_used=[],
                caveats=["data_source_unavailable" if depth != "quick" else "depth_skipped"],
            )
        )

    catalysts: list[CatalystEvent] = []
    # Surface close_time as a known catalyst boundary (not a news scrape).
    catalysts.append(
        CatalystEvent(
            description="Contract close / resolution window end (from Kalshi close_time)",
            occurs_at=req.close_time,
            source="kalshi:close_time",
        )
    )
    if depth != "quick":
        catalysts.extend(news.extract_catalysts_from_request(req))

    elapsed_ms = int((time.perf_counter() - t0) * 1000)
    return MarketOpinionResponse(
        signals=signals,
        catalysts=catalysts,
        elapsed_ms=elapsed_ms,
    )


# Silence unused import warning if timezone needed later
_ = datetime, timezone
