# Agent orchestrator — fan-out to available estimators (plan §5, §7 Phase 2)
# Copyright (C) 2026 Ethan Jones
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Depth tiers (Sprint 3 / plan §7):
#   quick    — contract_tape only (board scan; no yfinance / news / macro)
#   standard — technical + contract_tape + news + macro (default Analyze / chat)
#   deep     — same live agents + full history path
#
# Explicitly NOT shipped yet:
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

from . import contract_tape, macro, news, technical


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
        # Board scan: tape only — skip yfinance / news / macro.
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
        signals.append(
            _null_signal(
                "macro",
                "depth=quick: macro (FRED) skipped for board scan latency.",
            )
        )
    else:
        # standard + deep: full live estimators
        signals.append(await technical.estimate(req))
        signals.append(await contract_tape.estimate(req))
        signals.append(await news.estimate(req))
        signals.append(await macro.estimate(req))

    # Sentiment still stubbed (no free non-AGPL feed wired).
    signals.append(
        AgentSignal(
            agent="sentiment",
            probability=None,
            confidence=0.0,
            rationale=(
                "depth=quick: sentiment skipped."
                if depth == "quick"
                else "Sentiment agent requires social/news sentiment feeds; not wired."
            ),
            inputs_used=[],
            caveats=["depth_skipped" if depth == "quick" else "data_source_unavailable"],
        )
    )

    catalysts: list[CatalystEvent] = []
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


_ = datetime, timezone
